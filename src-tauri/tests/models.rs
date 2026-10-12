use memechan_core::{
    intelligence::{import_local, missing_ocr, EmbeddingModel, Intelligence, OCR_FILES},
    Result,
};
use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

#[test]
#[ignore = "runs offline CPU inference with existing files; set MEMECHAN_MODEL_TEST_DIR"]
fn local_models_import_safely_and_support_ocr() -> Result<()> {
    let source = PathBuf::from(std::env::var("MEMECHAN_MODEL_TEST_DIR")?);
    let directory = tempfile::tempdir()?;
    let paths: Vec<_> = OCR_FILES
        .iter()
        .map(|file| source.join(file.name))
        .collect();
    let cancel = AtomicBool::new(false);
    let destination = directory.path().join("models");
    let model = EmbeddingModel::default();
    // Accept original repository filenames, and allow installing in several batches.
    let renamed = directory.path().join("angle-classifier.onnx");
    fs::copy(source.join("ocr-cls.onnx"), &renamed)?;
    import_local(
        &destination,
        model,
        std::slice::from_ref(&renamed),
        &cancel,
        |_, _, _| {},
    )?;
    assert!(renamed.exists());
    assert_eq!(missing_ocr(&destination).len(), OCR_FILES.len() - 1);
    // Reject a mixed selection before writing any of its files.
    let invalid = directory.path().join("tokenizer.json");
    fs::write(&invalid, "{}")?;
    assert!(import_local(
        &destination,
        model,
        &[paths[0].clone(), invalid],
        &cancel,
        |_, _, _| {}
    )
    .is_err());
    assert!(!destination.join("ocr-v5-det.onnx").exists());
    import_local(&destination, model, &paths, &cancel, |_, _, _| {})?;
    assert!(missing_ocr(&destination).is_empty());
    // Re-import from the installation itself without overwriting the source.
    import_local(
        &destination,
        model,
        &[destination.join("ocr-cls.onnx")],
        &cancel,
        |_, _, _| {},
    )?;
    let mut models = Intelligence::load(&destination, model);
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr.png");
    let recognized = models.recognize(&fixture)?;
    assert!(
        recognized.contains("今天不想上班"),
        "OCR text: {recognized}"
    );
    Ok(())
}

#[test]
#[ignore = "offline CPU inference; set MEMECHAN_MODEL_TEST_DIR and optionally MEMECHAN_EMBEDDING_MODEL"]
fn selected_qwen_model_supports_multilingual_search() -> Result<()> {
    let source = PathBuf::from(std::env::var("MEMECHAN_MODEL_TEST_DIR")?);
    let name = std::env::var("MEMECHAN_EMBEDDING_MODEL").unwrap_or_else(|_| "qwen3-0.6b".into());
    let model: EmbeddingModel = serde_json::from_value(serde_json::Value::String(name))?;
    // Keep multi-GB test copies on the source filesystem, rather than /tmp's RAM disk.
    let installed = tempfile::tempdir_in(source.parent().unwrap())?;
    import_local(
        installed.path(),
        model,
        &[source.join("memechan-model.json")],
        &AtomicBool::new(false),
        |_, _, _| {},
    )?;
    let mut models = Intelligence::load(installed.path(), model);
    let related = models.embed("上班很累，我只想摸鱼早点下班", false)?;
    let unrelated = models.embed("生日快乐，祝你生日开心", false)?;
    for text in [
        "我不想继续工作，想下班休息",
        "I want to leave work and rest",
    ] {
        let query = models.embed(text, true)?;
        assert_eq!(query.len(), model.dimensions());
        assert!((query.iter().map(|value| value * value).sum::<f32>() - 1.0).abs() < 0.001);
        let similarity = |other: &[f32]| query.iter().zip(other).map(|(a, b)| a * b).sum::<f32>();
        assert!(
            similarity(&related) > similarity(&unrelated),
            "query: {text}"
        );
    }
    Ok(())
}
