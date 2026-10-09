use memechan_core::{
    intelligence::{import_local, missing, Intelligence, EMBEDDING_DIMENSIONS, FILES},
    Result,
};
use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

#[test]
#[ignore = "runs offline CPU inference with existing files; set MEMECHAN_MODEL_TEST_DIR"]
fn local_models_import_safely_and_support_ocr_and_semantic_search() -> Result<()> {
    let source = PathBuf::from(std::env::var("MEMECHAN_MODEL_TEST_DIR")?);
    let directory = tempfile::tempdir()?;
    let paths: Vec<_> = FILES.iter().map(|file| source.join(file.name)).collect();
    let cancel = AtomicBool::new(false);
    let destination = directory.path().join("models");
    // Accept original repository filenames, and allow installing in several batches.
    let renamed = directory.path().join("model_quantized.onnx");
    fs::copy(source.join("qwen3-embedding-0.6b.onnx"), &renamed)?;
    import_local(
        &destination,
        std::slice::from_ref(&renamed),
        &cancel,
        |_, _, _| {},
    )?;
    assert!(renamed.exists());
    assert_eq!(missing(&destination).len(), FILES.len() - 1);
    // Reject a mixed selection before writing any of its files.
    let invalid = directory.path().join("tokenizer.json");
    fs::write(&invalid, "{}")?;
    assert!(import_local(
        &destination,
        &[paths[0].clone(), invalid],
        &cancel,
        |_, _, _| {}
    )
    .is_err());
    assert!(!destination.join("ocr-v5-det.onnx").exists());
    import_local(&destination, &paths, &cancel, |_, _, _| {})?;
    assert!(missing(&destination).is_empty());
    // Re-import from the installation itself without rehashing all large weights.
    import_local(
        &destination,
        &[destination.join("qwen3-tokenizer.json")],
        &cancel,
        |_, _, _| {},
    )?;
    let mut models = Intelligence::load(&destination)?;
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr.png");
    let recognized = models.recognize(&fixture)?;
    assert!(
        recognized.contains("今天不想上班"),
        "OCR text: {recognized}"
    );
    let related = models.embed("上班很累，我只想摸鱼早点下班", false)?;
    let unrelated = models.embed("生日快乐，祝你生日开心", false)?;
    for text in [
        "我不想继续工作，想下班休息",
        "I want to leave work and rest",
    ] {
        let query = models.embed(text, true)?;
        assert_eq!(query.len(), EMBEDDING_DIMENSIONS);
        assert!((query.iter().map(|value| value * value).sum::<f32>() - 1.0).abs() < 0.001);
        let similarity = |other: &[f32]| query.iter().zip(other).map(|(a, b)| a * b).sum::<f32>();
        assert!(
            similarity(&related) > similarity(&unrelated),
            "query: {text}"
        );
    }
    Ok(())
}
