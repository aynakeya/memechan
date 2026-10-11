use super::*;

#[test]
fn disabling_semantic_search_cancels_work_and_preserves_indexes() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = Arc::new(AppState::new(directory.path())?);
    let path = directory.path().join("敬礼.png");
    image::RgbaImage::new(4, 4).save(&path)?;
    state.import_paths(&[path.to_string_lossy().into_owned()], None)?;
    {
        let mut library = state.library_write.lock().unwrap();
        let asset = library.assets()?.remove(0);
        library.put_embedding(&asset.id, &Library::search_text(&asset), &[1.0, 0.0])?;
    }
    let generation = state.invalidate_searches();
    // Hold an in-flight model operation: disabling must persist first, then wait for release.
    let models = state.intelligence.lock().unwrap();
    let disabling = {
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            state.save_settings(Settings {
                semantic_enabled: false,
                ..Default::default()
            })
        })
    };
    wait_until(|| {
        !state
            .library_read
            .lock()
            .unwrap()
            .settings()
            .unwrap()
            .semantic_enabled
    })?;
    wait_until(|| !state.inference_scheduler.is_search_current(generation))?;
    assert!(state.search(generation, "敬礼", "hybrid", None)?.is_none());
    assert_eq!(
        state
            .search(state.invalidate_searches(), "敬礼", "hybrid", None)?
            .unwrap()
            .len(),
        1
    );
    let queued = {
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            state.with_models(TaskKind::Index, |_| -> Result<()> {
                panic!("disabled model must not run")
            })
        })
    };
    drop(models);
    disabling.join().unwrap()?;
    assert!(queued.join().unwrap()?.is_none());
    assert!(state
        .search(state.invalidate_searches(), "敬礼", "semantic", None)
        .is_err());
    assert!(state
        .control_task(TaskKind::Index, TaskAction::Resume, None)
        .is_err());
    assert_eq!(state.model_status()?.index.completed, 1);
    assert_eq!(state.model_status()?.index.failed, 0);
    drop(state);
    let reopened = Arc::new(AppState::new(directory.path())?);
    reopened.auto_index()?;
    let settings = reopened.library_read.lock().unwrap().settings()?;
    assert!(!settings.semantic_enabled && settings.auto_index);
    let status = reopened.model_status()?;
    assert!(!status.semantic_enabled && !status.index.running);
    assert_eq!(status.index.completed, 1);
    Ok(())
}

#[test]
fn committed_changes_are_readable_while_the_writer_is_occupied() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = Arc::new(AppState::new(directory.path())?);
    let before = state.model_status()?.library_revision;
    let mut writer = state.library_write.lock().unwrap();
    let path = directory.path().join("敬礼.png");
    image::RgbaImage::new(4, 4).save(&path)?;
    assert_eq!(
        writer
            .import_files(&[path.to_string_lossy().into_owned()], None)
            .added,
        1
    );

    let (send, receive) = std::sync::mpsc::channel();
    let reader = Arc::clone(&state);
    let thread = std::thread::spawn(move || {
        send.send((|| -> Result<_> {
            let status = reader.model_status()?;
            let snapshot = reader.library_read.lock().unwrap().snapshot()?;
            let found = reader.search(reader.invalidate_searches(), "敬礼", "keyword", None)?;
            Ok((status.library_revision, snapshot, found))
        })())
        .unwrap();
    });
    let result = receive.recv_timeout(std::time::Duration::from_secs(5));
    drop(writer);
    thread.join().unwrap();
    let (revision, snapshot, found) = result??;
    assert_ne!(revision, before);
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.assets.len(), 1);
    assert_eq!(found.unwrap().len(), 1);
    assert_eq!(state.model_status()?.library_revision, revision);
    // Runtime progress must stay live when database counts are cached.
    {
        let mut local = state.local.lock().unwrap();
        local.ocr.running = true;
        local.ocr.paused = true;
        local.ocr.done = 3;
        local.ocr.message = "正在识别".into();
    }
    let status = state.model_status()?;
    assert_eq!(status.library_revision, revision);
    assert!(status.ocr.running && status.ocr.paused);
    assert_eq!(status.ocr.done, 3);
    assert_eq!(status.ocr.message, "正在识别");
    assert_eq!(status.ocr.pending, 1);
    assert_eq!(status.index.pending, 1);
    assert_eq!(status.vision.incomplete, 1);
    // The read connection must also reject accidental writes at the database boundary.
    assert!(state
        .library_read
        .lock()
        .unwrap()
        .save_group(None, "禁止写入", "#88966d")
        .is_err());
    Ok(())
}

#[test]
fn invalid_embedding_manifest_does_not_block_library_operations() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let source = directory.path().join("敬礼.png");
    image::RgbaImage::new(4, 4).save(&source)?;
    let paths = [source.to_string_lossy().into_owned()];
    let state = Arc::new(AppState::new(&root)?);
    assert_eq!(state.import_paths(&paths, None)?.added, 1);
    let id = {
        let mut library = state.library_write.lock().unwrap();
        let asset = library.assets()?.remove(0);
        library.set_embedding_version("installed-model-fingerprint")?;
        library.put_embedding(&asset.id, &Library::search_text(&asset), &[1.0, 0.0])?;
        asset.id
    };
    drop(state);

    let model_directory = root
        .join("models")
        .join(intelligence::EmbeddingModel::default().directory());
    std::fs::create_dir_all(&model_directory)?;
    for manifest in ["{", "{}"] {
        std::fs::write(model_directory.join("memechan-model.json"), manifest)?;
        let state = Arc::new(AppState::new(&root)?);
        assert!(!state.model_status()?.ready);
        assert!(state
            .model_status()?
            .missing
            .iter()
            .any(|error| error.contains("模型清单无效")));
        assert_eq!(
            state
                .search(state.invalidate_searches(), "敬礼", "keyword", None)?
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            state
                .search(state.invalidate_searches(), "敬礼", "hybrid", None)?
                .unwrap()
                .len(),
            1
        );
        assert!(state
            .search(state.invalidate_searches(), "敬礼", "semantic", None)
            .is_err());
        assert_eq!(state.import_paths(&paths, None)?.duplicates, 1);
        let another = directory
            .path()
            .join(format!("导入-{}.png", manifest.len()));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([manifest.len() as u8, 0, 0, 255]))
            .save(&another)?;
        assert_eq!(
            state
                .import_paths(&[another.to_string_lossy().into_owned()], None)?
                .added,
            1
        );
        state.save_settings(Settings {
            copy_size: 320,
            ..Default::default()
        })?;
        let library = state.library_read.lock().unwrap();
        assert_eq!(library.snapshot()?.settings.copy_size, 320);
        let asset = library.asset(&id)?;
        assert_eq!(asset.index_state, "ready");
        assert_eq!(library.embedding_version()?, "installed-model-fingerprint");
        assert!(crate::media::prepare_copy(&asset, &root, 0, false)?.is_file());
    }
    Ok(())
}

#[test]
fn vision_queue_pauses_retries_and_discards_cleared_inflight_results() -> Result<()> {
    use crate::test_support::{completion, server};
    use std::sync::mpsc;
    let (started_tx, started_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let (base_url, server) = server(
        vec![
            (200, completion("女孩敬礼")),
            (200, completion("企鹅打滚")),
            (429, serde_json::json!({"error":{"message":"rate limit"}})),
            (200, completion("小猫哭泣")),
        ],
        move |index| {
            if index == 0 {
                started_tx.send(()).unwrap();
                continue_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
            }
        },
    )?;
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let config = VisionConfig {
        base_url,
        auto_describe: true,
        api_key: "test-key".into(),
        ..Default::default()
    };
    let state = Arc::new(AppState::new(&root)?);
    state.save_vision_config(config)?;
    let mut paths = vec![];
    for index in 0..3 {
        let path = directory.path().join(format!("meme-{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 0, 0, 255])).save(&path)?;
        paths.push(path.to_string_lossy().into_owned());
    }
    state.import_paths(&paths[..2], None)?;
    started_rx.recv_timeout(std::time::Duration::from_secs(10))?;
    let first = state
        .library_read
        .lock()
        .unwrap()
        .next_vision(None)?
        .unwrap();
    // HTTP is in flight; editing must still work and must survive the response.
    state
        .library_write
        .lock()
        .unwrap()
        .save_asset(&first.id, "我的敬礼描述", true, &[])?;
    state.clear_text(TaskKind::Vision, Some(&first.id))?;
    state.control_task(TaskKind::Vision, TaskAction::Pause, None)?;
    continue_tx.send(())?;
    wait_until(|| !state.model_status().unwrap().vision.running)?;
    assert_eq!(state.model_status().unwrap().vision.done, 1);
    assert_eq!(
        state
            .library_read
            .lock()
            .unwrap()
            .asset(&first.id)?
            .description,
        "我的敬礼描述"
    );
    assert_eq!(
        state
            .library_read
            .lock()
            .unwrap()
            .search("敬礼", "keyword", None, None)?
            .len(),
        1
    );
    state.import_paths(&paths[2..], None)?;
    assert!(!state.model_status().unwrap().vision.running);
    assert_eq!(
        state
            .library_read
            .lock()
            .unwrap()
            .task_counts(TaskKind::Vision)?
            .0,
        2
    );
    state.control_task(TaskKind::Vision, TaskAction::Resume, None)?;
    wait_until(|| !state.model_status().unwrap().vision.running)?;
    assert!(state
        .vision
        .lock()
        .unwrap()
        .job
        .error
        .as_ref()
        .unwrap()
        .contains("429"));
    state.control_task(TaskKind::Vision, TaskAction::Retry, None)?;
    wait_until(|| !state.model_status().unwrap().vision.running)?;
    assert_eq!(
        state
            .library_read
            .lock()
            .unwrap()
            .task_counts(TaskKind::Vision)?
            .0,
        0
    );
    assert!(state
        .library_read
        .lock()
        .unwrap()
        .assets()?
        .iter()
        .all(|asset| asset.ai_state
            == if asset.id == first.id {
                "idle"
            } else {
                "ready"
            }));
    assert_eq!(server.join().unwrap().len(), 4);
    drop(state);
    let library = Library::open(&root)?;
    assert!(library.asset(&first.id)?.ai_description.is_empty());
    Ok(())
}

#[test]
fn vision_config_survives_restart_and_can_be_edited_or_cleared() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let state = Arc::new(AppState::new(root)?);
    assert!(!state.vision_status()?.active);
    // Settings written before key persistence remain readable.
    let mut config: VisionConfig = serde_json::from_value(serde_json::json!({
        "baseUrl": "http://localhost:8080/v1",
        "model": "test-vision",
        "autoDescribe": false
    }))?;
    assert!(config.api_key.is_empty());
    config.api_key = "test-original-key".into();
    state.save_vision_config(config)?;
    drop(state);

    for key in ["test-original-key", "test-updated-key", ""] {
        let state = Arc::new(AppState::new(root)?);
        let mut config = state.vision_status()?.config;
        config.api_key = key.into();
        state.save_vision_config(config)?;
        drop(state);
        let reopened = AppState::new(root)?;
        let status = reopened.vision_status()?;
        assert!(status.active);
        assert_eq!(status.config.api_key, key);
        assert_eq!(status.config.base_url, "http://localhost:8080/v1");
        assert_eq!(status.config.model, "test-vision");
        assert!(!status.config.auto_describe);
    }
    Ok(())
}
use std::time::{Duration, Instant};

fn wait_until(ready: impl Fn() -> bool) -> Result<()> {
    let start = Instant::now();
    while !ready() {
        if start.elapsed() > Duration::from_secs(60) {
            return Err("后台索引测试超时".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[test]
#[ignore = "offline model inference; set MEMECHAN_MODEL_TEST_DIR"]
fn indexing_and_search_work_with_ocr_disabled_and_only_qwen_installed() -> Result<()> {
    let source = PathBuf::from(std::env::var("MEMECHAN_MODEL_TEST_DIR")?);
    let directory = tempfile::tempdir_in(source.parent().unwrap())?;
    let state = Arc::new(AppState::new(directory.path())?);
    state.start_install(vec![source.join("memechan-model.json")])?;
    wait_until(|| !state.job.lock().unwrap().running)?;
    assert!(state.job.lock().unwrap().error.is_none());
    state.save_settings(Settings {
        ocr_enabled: false,
        auto_index: false,
        ..Default::default()
    })?;
    let path = directory.path().join("meme.png");
    image::RgbaImage::new(4, 4).save(&path)?;
    state.import_paths(&[path.to_string_lossy().into_owned()], None)?;
    let id = {
        let mut library = state.library_write.lock().unwrap();
        let asset = library.assets()?.remove(0);
        library.save_asset(&asset.id, "开心敬礼", false, &[])?;
        asset.id
    };
    let other = directory.path().join("other.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 0, 0, 255])).save(&other)?;
    state.import_paths(&[other.to_string_lossy().into_owned()], None)?;
    state.control_task(TaskKind::Index, TaskAction::Generate, Some(&id))?;
    wait_until(|| !state.local.lock().unwrap().worker_running)?;
    let status = state.model_status()?;
    assert!(status.ready && !status.ocr_ready && !status.ocr_enabled);
    assert!(status.index.error.is_none());
    let asset = state.library_read.lock().unwrap().asset(&id)?;
    assert_eq!(asset.ocr_state, "pending");
    assert_eq!(asset.index_state, "ready");
    assert_eq!(status.index.pending, 1);
    assert_eq!(
        state
            .search(state.invalidate_searches(), "salute", "semantic", None)?
            .unwrap()[0]
            .id,
        id
    );
    state.control_task(TaskKind::Index, TaskAction::Resume, None)?;
    wait_until(|| !state.local.lock().unwrap().worker_running)?;
    assert_eq!(state.model_status()?.index.pending, 0);
    assert!(state.model_status()?.index.message.is_empty());

    // Disabling releases the loaded embedding model without clearing stored vectors.
    state.save_settings(Settings {
        ocr_enabled: false,
        semantic_enabled: false,
        ..Default::default()
    })?;
    assert_eq!(state.model_status()?.index.completed, 2);
    assert_eq!(
        state
            .search(state.invalidate_searches(), "开心敬礼", "hybrid", None)?
            .unwrap()[0]
            .id,
        id
    );
    assert!(state
        .search(state.invalidate_searches(), "salute", "semantic", None)
        .is_err());
    state.save_settings(Settings {
        ocr_enabled: false,
        auto_index: false,
        ..Default::default()
    })?;
    assert_eq!(
        state
            .search(state.invalidate_searches(), "salute", "semantic", None)?
            .unwrap()[0]
            .id,
        id
    );

    // Clearing during inference also survives automatic scheduling afterwards.
    state.save_settings(Settings {
        ocr_enabled: false,
        ..Default::default()
    })?;
    let inference = state.intelligence.lock().unwrap();
    state.control_task(TaskKind::Index, TaskAction::Generate, Some(&id))?;
    wait_until(|| !state.local.lock().unwrap().index.message.is_empty())?;
    assert!(state
        .save_settings(Settings {
            embedding_model: intelligence::EmbeddingModel::Qwen4B,
            ..Default::default()
        })
        .is_err());
    state.clear_text(TaskKind::Index, None)?;
    drop(inference);
    wait_until(|| !state.local.lock().unwrap().worker_running)?;
    state.auto_index()?;
    let status = state.model_status()?;
    assert_eq!(
        (
            status.index.completed,
            status.index.incomplete,
            status.index.pending
        ),
        (0, 2, 0)
    );
    assert!(!status.index.running);
    assert!(state
        .search(state.invalidate_searches(), "salute", "semantic", None)?
        .unwrap()
        .is_empty());
    state.control_task(TaskKind::Index, TaskAction::Generate, None)?;
    wait_until(|| !state.local.lock().unwrap().worker_running)?;
    assert_eq!(state.model_status()?.index.completed, 2);
    state.save_settings(Settings {
        ocr_enabled: false,
        auto_index: false,
        embedding_model: intelligence::EmbeddingModel::Qwen4B,
        ..Default::default()
    })?;
    assert!(state.intelligence.lock().unwrap().is_none());
    let status = state.model_status()?;
    assert!(!status.ready);
    assert_eq!(status.index.pending, 2);
    assert_eq!(status.index.completed, 0);
    state.save_settings(Settings {
        ocr_enabled: false,
        ..Default::default()
    })?;
    wait_until(|| !state.local.lock().unwrap().worker_running)?;
    assert_eq!(state.model_status()?.index.completed, 2);
    assert_eq!(
        state
            .search(state.invalidate_searches(), "salute", "semantic", None)?
            .unwrap()[0]
            .id,
        id
    );
    Ok(())
}

#[test]
#[ignore = "offline model inference; set MEMECHAN_MODEL_TEST_DIR"]
fn bulk_import_joins_active_worker_and_resumes_after_restart() -> Result<()> {
    let source = PathBuf::from(std::env::var("MEMECHAN_MODEL_TEST_DIR")?);
    let directory = tempfile::tempdir_in(source.parent().unwrap())?;
    let root = directory.path().join("library");
    let models = root.join("models");
    for paths in [
        vec![source.join("memechan-model.json")],
        intelligence::OCR_FILES
            .iter()
            .map(|file| source.join(file.name))
            .collect(),
    ] {
        intelligence::import_local(
            &models,
            Default::default(),
            &paths,
            &AtomicBool::new(false),
            |_, _, _| {},
        )?;
    }
    let state = Arc::new(AppState::new(&root)?);
    state.control_task(TaskKind::Ocr, TaskAction::Pause, None)?;
    state.control_task(TaskKind::Index, TaskAction::Pause, None)?;
    let mut paths = Vec::new();
    for index in 0..32 {
        let path = directory.path().join(format!("meme-{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 100, 150, 255])).save(&path)?;
        paths.push(path.to_string_lossy().into_owned());
    }
    assert_eq!(state.import_paths(&paths[..1], None)?.added, 1);
    let first = state
        .library_read
        .lock()
        .unwrap()
        .next_task(TaskKind::Index, false, None)?
        .unwrap();
    // Hold inference while a single-item job starts, then append a bulk import.
    let inference = state.intelligence.lock().unwrap();
    state.control_task(TaskKind::Ocr, TaskAction::Generate, Some(&first.id))?;
    state.control_task(TaskKind::Index, TaskAction::Resume, None)?;
    assert_eq!(state.import_paths(&paths[1..], None)?.added, 31);
    assert_eq!(
        state.library_read.lock().unwrap().snapshot()?.assets.len(),
        32
    );
    wait_until(|| state.local.lock().unwrap().ocr.message == first.name)?;
    state.clear_text(TaskKind::Ocr, Some(&first.id))?;
    drop(inference);

    wait_until(|| state.local.lock().unwrap().index.done > 0)?;
    state.control_task(TaskKind::Ocr, TaskAction::Pause, None)?;
    state.control_task(TaskKind::Index, TaskAction::Pause, None)?;
    wait_until(|| !state.local.lock().unwrap().worker_running)?;
    let ready = state.library_read.lock().unwrap().asset(&first.id)?;
    assert_eq!(ready.index_state, "ready");
    assert_eq!(ready.ocr_state, "idle");
    let remaining = state
        .library_read
        .lock()
        .unwrap()
        .task_counts(TaskKind::Index)?
        .0;
    assert!(remaining > 0 && remaining < 32);
    // New imports cannot silently unpause an explicitly paused queue.
    let extra = directory.path().join("extra.png");
    image::RgbaImage::new(4, 4).save(&extra)?;
    assert_eq!(
        state
            .import_paths(&[extra.to_string_lossy().into_owned()], None)?
            .added,
        1
    );
    assert!(!state.local.lock().unwrap().worker_running);
    let broken = state
        .library_read
        .lock()
        .unwrap()
        .assets()?
        .into_iter()
        .find(|asset| asset.ocr_state == "pending")
        .unwrap();
    std::fs::write(&broken.path, b"invalid image")?;

    let intelligence = state.intelligence.lock().unwrap().take();
    drop(state);
    let restarted = Arc::new(AppState {
        intelligence: Mutex::new(intelligence),
        ..AppState::new(&root)?
    });
    restarted.auto_index()?;
    wait_until(|| !restarted.local.lock().unwrap().worker_running)?;
    let library = restarted.library_read.lock().unwrap();
    assert_eq!(library.task_counts(TaskKind::Index)?.0, 0);
    assert_eq!(library.asset(&broken.id)?.ocr_state, "error");
    assert_eq!(
        library
            .assets()?
            .iter()
            .filter(|a| a.index_state == "ready")
            .count(),
        33
    );
    assert!(restarted.local.lock().unwrap().ocr.error.is_some());
    assert_eq!(restarted.local.lock().unwrap().index.done, remaining + 1);
    drop(library);

    // A bulk arrival during a targeted reindex must be picked up without a restart.
    let inference = restarted.intelligence.lock().unwrap();
    restarted.control_task(TaskKind::Index, TaskAction::Generate, Some(&first.id))?;
    let arrived = directory.path().join("arrived.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 23, 255])).save(&arrived)?;
    assert_eq!(
        restarted
            .import_paths(&[arrived.to_string_lossy().into_owned()], None)?
            .added,
        1
    );
    drop(inference);
    wait_until(|| !restarted.local.lock().unwrap().worker_running)?;
    assert_eq!(
        restarted
            .library_read
            .lock()
            .unwrap()
            .task_counts(TaskKind::Index)?
            .0,
        0
    );
    assert_eq!(restarted.local.lock().unwrap().index.done, 2);
    Ok(())
}
