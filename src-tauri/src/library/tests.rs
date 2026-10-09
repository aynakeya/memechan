use super::*;

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
    library.queue_vision(None)?;
    assert_eq!(library.vision_pending_count()?, 1);
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
    library.queue_vision(None)?;
    assert_eq!(library.vision_pending_count()?, 0);
    library.queue_vision(Some(&asset.id))?;
    library.fail_vision(&asset.id, "HTTP 429")?;
    assert_eq!(library.asset(&asset.id)?.ai_description, description.text);
    library.queue_vision(None)?;
    assert_eq!(library.vision_pending_count()?, 1);
    library.delete_asset(&asset.id)?;
    library.update_vision(&asset.id, &description)?;
    assert!(library.assets()?.is_empty());
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
    library.delete_group(group.id)?;
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
    let asset = library.next_pending(None)?.unwrap();
    library.update_ocr(&asset.id, "收到")?;
    library.save_asset(&asset.id, "举手敬礼", true, &[])?;
    let text = Library::search_text(&library.asset(&asset.id)?);
    library.put_embedding(&asset.id, &text, &[1.0, 0.0])?;
    library
        .db
        .execute("UPDATE embeddings SET model='previous-model'", [])?;
    drop(library);

    let mut library = Library::open(&root)?;
    let pending = library.next_pending(None)?.unwrap();
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
    assert_eq!(Library::open(&root)?.pending_count(None)?, 0);
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
    let first = library.next_pending(None)?.unwrap();
    assert_eq!(first.name, "meme-0");
    library.update_ocr(&first.id, "开心")?;
    let text = Library::search_text(&library.asset(&first.id)?);
    library.put_embedding(&first.id, &text, &[1.0, 0.0])?;
    assert_eq!(library.pending_count(None)?, 3);
    assert_eq!(
        library
            .search("happy", "semantic", None, Some(&[1.0, 0.0]))?
            .len(),
        1
    );

    let failed = library.next_pending(None)?.unwrap();
    library.mark_error(&failed.id, true)?;
    library.mark_error(&failed.id, false)?;
    let next = library.next_pending(None)?.unwrap();
    assert_ne!(next.id, failed.id);
    drop(library);

    let mut library = Library::open(&root)?;
    assert_eq!(library.pending_count(None)?, 2);
    assert_eq!(library.next_pending(None)?.unwrap().id, next.id);
    assert_eq!(library.asset(&first.id)?.index_state, "ready");
    library.delete_asset(&next.id)?;
    // A job finishing after deletion must neither fail the queue nor resurrect the item.
    library.put_embedding(&next.id, &Library::search_text(&next), &[1.0, 0.0])?;
    assert!(library.find_asset(&next.id)?.is_none());
    assert_eq!(library.pending_count(None)?, 1);
    assert!(library.next_pending(Some(&first.id))?.is_none());
    Ok(())
}
