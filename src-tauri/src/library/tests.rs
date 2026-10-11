use super::*;

#[test]
fn clearing_indexes_preserves_text_and_waits_for_explicit_rebuild() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let mut library = Library::open(&root)?;
    for index in 0..2 {
        let path = directory.path().join(format!("{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 0, 0, 255])).save(&path)?;
        library.import_files(&[path.to_string_lossy().into_owned()], None);
    }
    for asset in library.assets()? {
        library.save_asset(&asset.id, "我的描述", true, &[])?;
        library.update_ocr(&asset.id, "收到")?;
        library.update_vision(
            &asset.id,
            &crate::vision::Description {
                text: "角色敬礼".into(),
                model: "test".into(),
            },
        )?;
        library.put_embedding(
            &asset.id,
            &Library::search_text(&library.asset(&asset.id)?),
            &[1.0, 0.0],
        )?;
    }
    let assets = library.assets()?;
    assert_eq!(library.clear_text(TaskKind::Index, None)?, 2);
    for asset in &assets {
        // A result already in flight must not restore the cleared vector.
        library.put_embedding(&asset.id, &Library::search_text(asset), &[1.0, 0.0])?;
        let current = library.asset(&asset.id)?;
        assert_eq!(current.description, asset.description);
        assert_eq!(current.ocr_text, asset.ocr_text);
        assert_eq!(current.ai_description, asset.ai_description);
        assert!(current.favorite && Path::new(&current.path).is_file());
        library.update_ocr(&asset.id, "收到新的文字")?;
        library.update_vision(
            &asset.id,
            &crate::vision::Description {
                text: "角色正在敬礼".into(),
                model: "test".into(),
            },
        )?;
        library.save_asset(&asset.id, "修改后的描述", true, &[])?;
    }
    assert!(library
        .search("salute", "semantic", None, Some(&[1.0, 0.0]))?
        .is_empty());
    assert_eq!(library.search("敬礼", "keyword", None, None)?.len(), 2);
    drop(library);

    let mut library = Library::open(&root)?;
    library.queue_task(TaskKind::Index, None, true)?;
    assert_eq!(library.task_counts(TaskKind::Index)?, (0, 0, 0, 2));
    library.queue_task(TaskKind::Index, None, false)?;
    assert_eq!(library.task_counts(TaskKind::Index)?, (2, 0, 0, 2));
    for asset in library.assets()? {
        library.put_embedding(&asset.id, &Library::search_text(&asset), &[1.0, 0.0])?;
    }
    assert_eq!(library.task_counts(TaskKind::Index)?, (0, 0, 2, 0));
    Ok(())
}

#[test]
fn clearing_generated_text_preserves_other_sources_and_retry_does_not_restore_it() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut library = Library::open(&directory.path().join("library"))?;
    let mut ids = Vec::new();
    for index in 0..2 {
        let path = directory.path().join(format!("{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 0, 0, 255])).save(&path)?;
        library.import_files(&[path.to_string_lossy().into_owned()], None);
        let asset = library.assets()?.remove(0);
        library.save_asset(&asset.id, "手写内容", true, &[])?;
        library.update_ocr(&asset.id, "图片文字")?;
        library.update_vision(
            &asset.id,
            &crate::vision::Description {
                text: "图片描述".into(),
                model: "test".into(),
            },
        )?;
        library.put_embedding(
            &asset.id,
            &Library::search_text(&library.asset(&asset.id)?),
            &[1.0, 0.0],
        )?;
        ids.push(asset.id);
    }
    for kind in [TaskKind::Ocr, TaskKind::Vision, TaskKind::Index] {
        assert_eq!(library.task_counts(kind)?, (0, 0, 2, 0));
    }
    assert_eq!(library.clear_text(TaskKind::Vision, Some(&ids[0]))?, 1);
    assert_eq!(library.task_counts(TaskKind::Vision)?, (0, 0, 1, 1));
    assert_eq!(library.asset(&ids[1])?.ai_description, "图片描述");
    assert_eq!(
        library
            .search("动作", "semantic", None, Some(&[1.0, 0.0]))?
            .len(),
        1
    );
    assert_eq!(library.clear_text(TaskKind::Ocr, None)?, 2);
    assert!(library
        .search("图片文字", "keyword", None, None)?
        .is_empty());
    assert!(library
        .search("动作", "semantic", None, Some(&[1.0, 0.0]))?
        .is_empty());
    library.fail_vision(&ids[1], "HTTP 429")?;
    assert_eq!(library.task_counts(TaskKind::Vision)?, (0, 1, 0, 2));
    library.queue_task(TaskKind::Vision, None, true)?;
    assert_eq!(library.asset(&ids[0])?.ai_state, "idle");
    assert_eq!(library.asset(&ids[1])?.ai_state, "pending");
    library.queue_task(TaskKind::Ocr, None, true)?;
    assert_eq!(library.task_counts(TaskKind::Ocr)?, (0, 0, 0, 2));
    for asset in library.assets()? {
        assert_eq!(asset.description, "手写内容");
        assert!(asset.favorite && Path::new(&asset.path).is_file());
        assert_eq!(asset.ocr_state, "idle");
    }
    drop(library);
    let library = Library::open(&directory.path().join("library"))?;
    assert_eq!(library.asset(&ids[0])?.ai_state, "idle");
    assert_eq!(library.task_counts(TaskKind::Vision)?, (1, 0, 0, 2));
    assert_eq!(library.task_counts(TaskKind::Ocr)?, (0, 0, 0, 2));
    assert_eq!(library.task_counts(TaskKind::Index)?, (2, 0, 0, 2));
    Ok(())
}

#[test]
fn search_ranks_valid_vectors_and_keywords_within_the_selected_group() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut library = Library::open(&directory.path().join("library"))?;
    let group = library.save_group(None, "动作", "#88966d")?;
    let mut ids = Vec::new();
    for (index, description, vector) in [
        (0, "开心敬礼", [1.0, 0.0]),
        (1, "收到长官", [0.8, 0.6]),
        (2, "敬礼失败", [0.0, 1.0]),
    ] {
        let path = directory.path().join(format!("meme-{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 0, 0, 255])).save(&path)?;
        library.import_files(&[path.to_string_lossy().into_owned()], None);
        let asset = library.assets()?.remove(0);
        library.save_asset(
            &asset.id,
            description,
            false,
            if index > 0 {
                std::slice::from_ref(&group.id)
            } else {
                &[]
            },
        )?;
        let source = Library::search_text(&library.asset(&asset.id)?);
        library.put_embedding(&asset.id, &source, &vector)?;
        ids.push(asset.id);
    }
    let query = [1.0, 0.0];
    let result = library.search("敬礼", "hybrid", None, Some(&query))?;
    assert_eq!(result[0].id, ids[0]);
    let result = library.search("敬礼", "semantic", Some(group.id), Some(&query))?;
    assert_eq!(
        result.iter().map(|asset| &asset.id).collect::<Vec<_>>(),
        vec![&ids[1], &ids[2]]
    );
    let result = library.search("敬礼", "keyword", Some(group.id), None)?;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, ids[2]);
    library.save_asset(&ids[1], "正在睡觉", false, &[group.id])?;
    let result = library.search("敬礼", "semantic", Some(group.id), Some(&query))?;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, ids[2]);
    Ok(())
}

#[test]
fn text_changes_preserve_other_sources_and_reject_stale_vectors() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let path = directory.path().join("meme.png");
    image::RgbaImage::new(4, 4).save(&path)?;
    let mut library = Library::open(&root)?;
    assert_eq!(
        library
            .import_files(&[path.to_string_lossy().into_owned()], None)
            .added,
        1
    );
    let asset = library.assets()?.remove(0);
    assert_eq!(asset.ai_state, "idle");
    library.save_asset(&asset.id, "收到老板", true, &[])?;
    library.update_ocr(&asset.id, "收到")?;
    let source = Library::search_text(&library.asset(&asset.id)?);
    library.put_embedding(&asset.id, &source, &[1.0, 0.0])?;
    drop(library);
    let mut library = Library::open(&root)?;
    assert_eq!(library.asset(&asset.id)?.index_state, "ready");
    for change in ["description", "ocr"] {
        let old_source = Library::search_text(&library.asset(&asset.id)?);
        if change == "description" {
            library.save_asset(&asset.id, "收到老板，准备下班", true, &[])?;
        } else {
            library.update_ocr(&asset.id, "今天不想上班")?;
        }
        assert!(library
            .search("休息", "semantic", None, Some(&[1.0, 0.0]))?
            .is_empty());
        library.put_embedding(&asset.id, &old_source, &[1.0, 0.0])?;
        assert_eq!(library.asset(&asset.id)?.index_state, "pending");
        let source = Library::search_text(&library.asset(&asset.id)?);
        library.put_embedding(&asset.id, &source, &[1.0, 0.0])?;
    }
    assert_eq!(library.search("不想上班", "keyword", None, None)?.len(), 1);
    library.queue_task(TaskKind::Vision, None, false)?;
    assert_eq!(library.task_counts(TaskKind::Vision)?.0, 1);
    let description = crate::vision::Description {
        text: "女孩举手敬礼".into(),
        model: "test".into(),
    };
    library.update_vision(&asset.id, &description)?;
    let updated = library.asset(&asset.id)?;
    assert_eq!(updated.description, "收到老板，准备下班");
    assert_eq!(updated.ocr_text, "今天不想上班");
    assert!(updated.favorite);
    assert_eq!(updated.index_state, "pending");
    assert_eq!(library.search("敬礼", "keyword", None, None)?.len(), 1);
    assert!(library
        .search("敬礼", "semantic", None, Some(&[1.0, 0.0]))?
        .is_empty());
    library.put_embedding(&asset.id, &source, &[1.0, 0.0])?;
    assert_eq!(library.asset(&asset.id)?.index_state, "pending");
    library.put_embedding(&asset.id, &Library::search_text(&updated), &[0.0, 1.0])?;
    library.queue_task(TaskKind::Vision, None, false)?;
    assert_eq!(library.task_counts(TaskKind::Vision)?.0, 0);
    library.queue_task(TaskKind::Vision, Some(&asset.id), false)?;
    library.fail_vision(&asset.id, "HTTP 429")?;
    assert_eq!(library.asset(&asset.id)?.ai_description, description.text);
    library.queue_task(TaskKind::Vision, None, false)?;
    assert_eq!(library.task_counts(TaskKind::Vision)?.0, 1);
    library.delete_asset(&asset.id)?;
    library.update_vision(&asset.id, &description)?;
    assert!(library.assets()?.is_empty());
    Ok(())
}

#[test]
fn batch_management_preserves_metadata_and_deletes_only_selected_assets() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut library = Library::open(&directory.path().join("library"))?;
    let first = library.save_group(None, "动作", "#88966d")?.id;
    let second = library.save_group(None, "日常", "#88966d")?.id;
    let sources: Vec<_> = (0..3)
        .map(|index| directory.path().join(format!("{index}.png")))
        .collect();
    for (index, source) in sources.iter().enumerate() {
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index as u8, 0, 0, 255])).save(source)?;
        assert_eq!(
            library
                .import_files(&[source.to_string_lossy().into_owned()], None)
                .added,
            1
        );
    }
    let assets = library.assets()?;
    let ids: Vec<_> = assets[..2].iter().map(|asset| asset.id.clone()).collect();
    for (index, asset) in assets.iter().enumerate() {
        library.save_asset(&asset.id, &format!("描述 {index}"), index == 0, &[first])?;
        library.update_ocr(&asset.id, "敬礼")?;
        library.put_embedding(
            &asset.id,
            &Library::search_text(&library.asset(&asset.id)?),
            &[1.0, 0.0],
        )?;
    }
    let invalid_ids = vec![ids[0].clone(), "missing".into()];
    assert!(library
        .set_assets_group(&invalid_ids, second, true)
        .is_err());
    assert_eq!(library.asset(&ids[0])?.groups, vec![first]);
    library.set_assets_group(&ids, second, true)?;
    library.set_assets_group(&ids, second, true)?;
    library.set_assets_group(&ids, first, false)?;
    for (index, id) in ids.iter().enumerate() {
        let asset = library.asset(id)?;
        assert_eq!(asset.groups, vec![second]);
        assert_eq!(asset.description, format!("描述 {index}"));
        assert_eq!(asset.favorite, index == 0);
        assert_eq!(asset.ocr_text, "敬礼");
        assert_eq!(asset.index_state, "ready");
    }
    assert_eq!(library.asset(&assets[2].id)?.groups, vec![first]);
    assert!(library.delete_assets(&invalid_ids).is_err());
    assert_eq!(library.assets()?.len(), 3);
    assert!(assets.iter().all(|asset| Path::new(&asset.path).exists()));
    let copy = library.root.join("copies").join("clipboard.png");
    fs::write(&copy, b"clipboard")?;
    library.delete_assets(&ids)?;
    assert_eq!(library.assets()?.len(), 1);
    assert!(assets[..2]
        .iter()
        .all(|asset| !Path::new(&asset.path).exists() && !Path::new(&asset.thumbnail).exists()));
    assert!(Path::new(&assets[2].path).exists());
    assert!(copy.exists() && sources.iter().all(|source| source.exists()));
    assert_eq!(
        library
            .search("salute", "semantic", None, Some(&[1.0, 0.0]))?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn importing_duplicates_preserves_membership_and_group_deletion_keeps_files() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("smile.png");
    image::RgbaImage::from_pixel(12, 8, image::Rgba([40, 180, 90, 255])).save(&source)?;
    let mut library = Library::open(&directory.path().join("library"))?;
    let group = library.save_group(None, "开心", "#88966d")?;
    let paths = vec![source.to_string_lossy().into_owned()];
    assert_eq!(library.import_files(&paths, None).added, 1);
    assert_eq!(library.import_files(&paths, Some(group.id)).duplicates, 1);
    assert_eq!(library.assets()?.len(), 1);
    assert_eq!(library.assets()?[0].groups, vec![group.id]);
    let other = directory.path().join("other.png");
    image::RgbaImage::new(4, 4).save(&other)?;
    let (id, bytes) = crate::import::read_source(&other)?;
    let prepared = PreparedImport::new(&library.root, &other, id, &bytes)?;
    // A group may be deleted while the new image is being prepared outside the lock.
    library.delete_group(group.id)?;
    assert!(library.insert_import(prepared, Some(group.id)).is_err());
    assert_eq!(library.assets()?.len(), 1);
    for directory in ["originals", "thumbnails"] {
        assert_eq!(fs::read_dir(library.root.join(directory))?.count(), 1);
    }
    let asset = library.assets()?.remove(0);
    assert!(asset.groups.is_empty());
    assert!(Path::new(&asset.path).exists());
    library.delete_asset(&asset.id)?;
    assert!(source.exists());
    assert!(library.assets()?.is_empty());
    assert!(!Path::new(&asset.path).exists());
    Ok(())
}

#[test]
fn ocr_upgrade_preserves_library_and_queues_reindexing_once() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let source = directory.path().join("meme.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255])).save(&source)?;
    let mut library = Library::open(&root)?;
    let group = library.save_group(None, "打工", "#88966d")?;
    let report = library.import_files(&[source.to_string_lossy().into_owned()], Some(group.id));
    assert!(report.errors.is_empty());
    let id = library.assets()?.remove(0).id;
    library.save_asset(&id, "摸鱼", true, &[group.id])?;
    library.update_ocr(&id, "今天不想上班")?;
    let text = Library::search_text(&library.asset(&id)?);
    library.put_embedding(&id, &text, &[1.0, 0.0])?;
    library.db.execute_batch(
        "DROP INDEX assets_vision_pending;
            ALTER TABLE assets DROP COLUMN ai_description;
            ALTER TABLE assets DROP COLUMN ai_state;
            ALTER TABLE assets DROP COLUMN ai_error;
            ALTER TABLE assets DROP COLUMN ai_model;
            PRAGMA user_version=1;",
    )?;
    drop(library);

    let mut library = Library::open(&root)?;
    let asset = library.asset(&id)?;
    assert_eq!(asset.description, "摸鱼");
    assert!(asset.favorite);
    assert_eq!(asset.groups, vec![group.id]);
    assert!(Path::new(&asset.path).exists());
    assert_eq!(asset.ocr_state, "pending");
    assert_eq!(asset.index_state, "pending");
    assert_eq!(library.search("不想上班", "keyword", None, None)?.len(), 1);

    library.update_ocr(&id, "今天不想上班，想休息")?;
    // The old vector must not be returned after OCR changes its source text.
    assert!(library
        .search("休息", "semantic", None, Some(&[1.0, 0.0]))?
        .is_empty());
    let text = Library::search_text(&library.asset(&id)?);
    library.put_embedding(&id, &text, &[1.0, 0.0])?;
    drop(library);
    let library = Library::open(&root)?;
    let asset = library.asset(&id)?;
    assert_eq!(asset.ocr_state, "ready");
    assert_eq!(asset.index_state, "ready");
    Ok(())
}

#[test]
fn model_changes_requeue_vectors_without_repeating_ocr() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let source = directory.path().join("meme.png");
    image::RgbaImage::new(4, 4).save(&source)?;
    let mut library = Library::open(&root)?;
    assert_eq!(
        library
            .import_files(&[source.to_string_lossy().into_owned()], None)
            .added,
        1
    );
    let asset = library.next_task(TaskKind::Index, false, None)?.unwrap();
    library.update_ocr(&asset.id, "收到")?;
    library.save_asset(&asset.id, "举手敬礼", true, &[])?;
    let text = Library::search_text(&library.asset(&asset.id)?);
    library.set_embedding_version("installed-model-fingerprint")?;
    library.put_embedding(&asset.id, &text, &[1.0, 0.0])?;
    // An existing database learns its version from stored vectors during migration.
    library.db.execute_batch("DROP TABLE index_model")?;
    drop(library);
    let library = Library::open(&root)?;
    assert_eq!(library.embedding_version, "installed-model-fingerprint");
    assert_eq!(library.asset(&asset.id)?.index_state, "ready");
    library
        .db
        .execute("UPDATE embeddings SET model='previous-model'", [])?;
    drop(library);

    let mut library = Library::open(&root)?;
    let pending = library.next_task(TaskKind::Index, false, None)?.unwrap();
    assert_eq!(pending.id, asset.id);
    assert_eq!(pending.ocr_state, "ready");
    assert_eq!(pending.ocr_text, "收到");
    assert!(pending.favorite);
    assert_eq!(pending.description, "举手敬礼");
    assert!(library
        .search("salute", "semantic", None, Some(&[1.0, 0.0]))?
        .is_empty());
    library.put_embedding(&pending.id, &text, &[0.0, 1.0])?;
    drop(library);
    let mut library = Library::open(&root)?;
    assert_eq!(library.task_counts(TaskKind::Index)?.0, 0);
    for model in [
        EmbeddingModel::Qwen4B,
        EmbeddingModel::Qwen8B,
        EmbeddingModel::Qwen06B,
    ] {
        library.save_settings(Settings {
            embedding_model: model,
            ..Default::default()
        })?;
        assert_eq!(library.task_counts(TaskKind::Index)?, (1, 0, 0, 1));
        assert!(library
            .search("salute", "semantic", None, Some(&[0.0, 1.0]))?
            .is_empty());
        let asset = library.asset(&pending.id)?;
        assert_eq!(asset.ocr_state, "ready");
        assert_eq!(asset.description, "举手敬礼");
        assert!(asset.favorite);
        library.put_embedding(&asset.id, &text, &[0.0, 1.0])?;
        drop(library);
        library = Library::open(&root)?;
        assert_eq!(library.settings()?.embedding_model, model);
        assert_eq!(
            library
                .search("salute", "semantic", None, Some(&[0.0, 1.0]))?
                .len(),
            1
        );
    }
    library.clear_text(TaskKind::Index, None)?;
    library.save_settings(Settings {
        embedding_model: EmbeddingModel::Qwen4B,
        ..Default::default()
    })?;
    assert_eq!(library.task_counts(TaskKind::Index)?, (0, 0, 0, 1));
    Ok(())
}

#[test]
fn batch_indexing_publishes_each_item_and_resumes_only_pending_work() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("library");
    let mut paths = Vec::new();
    for index in 0..4 {
        let path = directory.path().join(format!("meme-{index}.png"));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 0, 0, 255])).save(&path)?;
        paths.push(path.to_string_lossy().into_owned());
    }
    paths.push(
        directory
            .path()
            .join("missing.png")
            .to_string_lossy()
            .into_owned(),
    );
    let mut library = Library::open(&root)?;
    let report = library.import_files(&paths, None);
    assert_eq!(report.added, 4);
    assert_eq!(report.errors.len(), 1);
    let first = library.next_task(TaskKind::Index, false, None)?.unwrap();
    assert_eq!(first.name, "meme-0");
    library.update_ocr(&first.id, "开心")?;
    let text = Library::search_text(&library.asset(&first.id)?);
    library.put_embedding(&first.id, &text, &[1.0, 0.0])?;
    assert_eq!(library.task_counts(TaskKind::Index)?.0, 3);
    assert_eq!(
        library
            .search("happy", "semantic", None, Some(&[1.0, 0.0]))?
            .len(),
        1
    );

    let failed = library.next_task(TaskKind::Index, false, None)?.unwrap();
    library.mark_error(&failed.id, true)?;
    library.mark_error(&failed.id, false)?;
    let next = library.next_task(TaskKind::Index, false, None)?.unwrap();
    assert_ne!(next.id, failed.id);
    drop(library);

    let mut library = Library::open(&root)?;
    assert_eq!(library.task_counts(TaskKind::Index)?.0, 2);
    assert_eq!(
        library.next_task(TaskKind::Index, false, None)?.unwrap().id,
        next.id
    );
    assert_eq!(library.asset(&first.id)?.index_state, "ready");
    library.delete_asset(&next.id)?;
    // A job finishing after deletion must neither fail the queue nor resurrect the item.
    library.put_embedding(&next.id, &Library::search_text(&next), &[1.0, 0.0])?;
    assert!(library.find_asset(&next.id)?.is_none());
    assert_eq!(library.task_counts(TaskKind::Index)?.0, 1);
    Ok(())
}
