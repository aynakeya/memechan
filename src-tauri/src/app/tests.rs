use super::*;

#[test]
fn vision_queue_publishes_each_item_and_pauses_without_blocking_library_edits() -> Result<()> {
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
        ..Default::default()
    };
    let state = Arc::new(AppState::new(&root)?);
    state.save_vision_config(config, Some("test-key".into()))?;
    let mut paths = vec![];
    for index in 0..3 {
        let path = directory.path().join(format!("meme-{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 0, 0, 255])).save(&path)?;
        paths.push(path.to_string_lossy().into_owned());
    }
    state.import_paths(&paths[..2], None)?;
    started_rx.recv_timeout(std::time::Duration::from_secs(10))?;
    let first = state.library.lock().unwrap().next_vision(None)?.unwrap();
    // HTTP is in flight; editing must still work and must survive the response.
    state
        .library
        .lock()
        .unwrap()
        .save_asset(&first.id, "我的敬礼描述", true, &[])?;
    state.pause_vision();
    continue_tx.send(())?;
    wait_until(|| !state.model_status().vision.running)?;
    assert_eq!(state.model_status().vision.done, 1);
    assert_eq!(
        state.library.lock().unwrap().asset(&first.id)?.description,
        "我的敬礼描述"
    );
    assert_eq!(
        state
            .library
            .lock()
            .unwrap()
            .search("敬礼", "keyword", None, None)?
            .len(),
        1
    );
    state.import_paths(&paths[2..], None)?;
    assert!(!state.model_status().vision.running);
    assert_eq!(state.library.lock().unwrap().vision_pending_count()?, 2);
    state.start_vision(None, true)?;
    wait_until(|| !state.model_status().vision.running)?;
    assert!(state
        .vision
        .lock()
        .unwrap()
        .job
        .error
        .as_ref()
        .unwrap()
        .contains("429"));
    state.start_vision(None, true)?;
    wait_until(|| !state.model_status().vision.running)?;
    assert_eq!(state.library.lock().unwrap().vision_pending_count()?, 0);
    assert!(state
        .library
        .lock()
        .unwrap()
        .assets()?
        .iter()
        .all(|asset| asset.ai_state == "ready"));
    assert_eq!(server.join().unwrap().len(), 4);
    drop(state);
    let library = Library::open(&root)?;
    assert_eq!(library.asset(&first.id)?.ai_description, "女孩敬礼");
    assert!(!std::fs::read(root.join("library.sqlite3"))?
        .windows(8)
        .any(|bytes| bytes == b"test-key"));
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
fn bulk_import_joins_active_worker_and_resumes_after_restart() -> Result<()> {
    let models = PathBuf::from(std::env::var("MEMECHAN_MODEL_TEST_DIR")?);
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let state = Arc::new(AppState {
        models: models.clone(),
        intelligence: Mutex::new(Some(Intelligence::load(&models)?)),
        ..AppState::new(&root)?
    });
    state.pause_index();
    let mut paths = Vec::new();
    for index in 0..32 {
        let path = directory.path().join(format!("meme-{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 100, 150, 255])).save(&path)?;
        paths.push(path.to_string_lossy().into_owned());
    }
    assert_eq!(state.import_paths(&paths[..1], None)?.added, 1);
    let first = state.library.lock().unwrap().next_pending(None)?.unwrap();
    // Hold inference while a single-item job starts, then append a bulk import.
    let inference = state.intelligence.lock().unwrap();
    state.start_job(None, Some(first.id.clone()), true)?;
    assert_eq!(state.import_paths(&paths[1..], None)?.added, 31);
    assert_eq!(state.library.lock().unwrap().snapshot()?.assets.len(), 32);
    drop(inference);

    wait_until(|| state.job.lock().unwrap().done > 0)?;
    state.cancel.store(true, Ordering::Relaxed);
    wait_until(|| !state.job.lock().unwrap().running)?;
    let ready = state.library.lock().unwrap().asset(&first.id)?;
    assert_eq!(ready.index_state, "ready");
    let remaining = state.library.lock().unwrap().pending_count(None)?;
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
    assert!(!state.job.lock().unwrap().running);
    let broken = state
        .library
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
        models,
        intelligence: Mutex::new(intelligence),
        ..AppState::new(&root)?
    });
    restarted.auto_index()?;
    wait_until(|| !restarted.job.lock().unwrap().running)?;
    let library = restarted.library.lock().unwrap();
    assert_eq!(library.pending_count(None)?, 0);
    assert_eq!(library.asset(&broken.id)?.ocr_state, "error");
    assert_eq!(
        library
            .assets()?
            .iter()
            .filter(|a| a.index_state == "ready")
            .count(),
        33
    );
    assert!(restarted.job.lock().unwrap().error.is_some());
    assert_eq!(restarted.job.lock().unwrap().done, remaining + 1);
    drop(library);

    // A bulk arrival during a targeted reindex must be picked up without a restart.
    let inference = restarted.intelligence.lock().unwrap();
    restarted.start_job(None, Some(first.id), true)?;
    let arrived = directory.path().join("arrived.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 23, 255])).save(&arrived)?;
    assert_eq!(
        restarted
            .import_paths(&[arrived.to_string_lossy().into_owned()], None)?
            .added,
        1
    );
    drop(inference);
    wait_until(|| !restarted.job.lock().unwrap().running)?;
    assert_eq!(restarted.library.lock().unwrap().pending_count(None)?, 0);
    assert_eq!(restarted.job.lock().unwrap().done, 2);
    Ok(())
}
