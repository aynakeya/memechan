use super::{checksum, EmbeddingModel, MAX_MODEL_BYTES};
use crate::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const MANIFEST: &str = "memechan-model.json";

#[derive(Serialize, Deserialize)]
pub struct Bundle {
    pub model: EmbeddingModel,
    pub onnx: String,
    pub tokenizer: String,
    pub output: String,
    pub files: BTreeMap<String, String>,
}
impl Bundle {
    pub fn read(path: &Path, model: EmbeddingModel) -> Result<Self> {
        let bundle: Self = serde_json::from_slice(&fs::read(path)?)?;
        if bundle.model != model {
            return Err("模型清单与所选 Qwen 大小不一致".into());
        }
        if !bundle.files.contains_key(&bundle.onnx)
            || !bundle.files.contains_key(&bundle.tokenizer)
            || !matches!(
                bundle.output.as_str(),
                "last_hidden_state" | "token_embeddings"
            )
        {
            return Err("模型清单缺少 ONNX、tokenizer 或有效的 token 输出".into());
        }
        for (name, digest) in &bundle.files {
            // Export bundles use flat, portable filenames; never copy outside their directory.
            if name.is_empty()
                || matches!(name.as_str(), "." | ".." | MANIFEST)
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                || digest.len() != 64
                || !digest.bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err(format!("模型清单包含无效的文件名或 SHA-256：{name}").into());
            }
        }
        Ok(bundle)
    }
    pub fn installed(directory: &Path, model: EmbeddingModel) -> Result<Option<Self>> {
        let path = directory.join(model.directory()).join(MANIFEST);
        if path.exists() {
            Ok(Some(Self::read(&path, model)?))
        } else {
            Ok(None)
        }
    }
    pub fn fingerprint(&self) -> Result<String> {
        Ok(Sha256::digest(serde_json::to_vec(self)?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    pub fn directory(&self, root: &Path) -> Result<PathBuf> {
        Ok(root.join(self.model.directory()).join(self.fingerprint()?))
    }
    pub fn verify(&self, directory: &Path) -> Result<()> {
        for (name, expected) in &self.files {
            if checksum(&directory.join(name))? != expected.to_lowercase() {
                return Err(format!("{name} 校验失败，请重新导入模型包").into());
            }
        }
        Ok(())
    }
}

pub fn import(
    root: &Path,
    model: EmbeddingModel,
    manifest: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize, String),
) -> Result<()> {
    let bundle = Bundle::read(manifest, model)?;
    let source = manifest.parent().unwrap();
    let target = bundle.directory(root)?;
    fs::create_dir_all(&target)?;
    for (index, (name, expected)) in bundle.files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let path = source.join(name);
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() || metadata.len() > MAX_MODEL_BYTES {
            return Err(format!("{name} 不是支持的模型文件").into());
        }
        progress(index, bundle.files.len(), format!("校验并导入 {name}"));
        let destination = target.join(name);
        if destination.is_file() && checksum(&destination)? == expected.to_lowercase() {
            continue;
        }
        let temporary = target.join(format!("{name}.part"));
        fs::copy(&path, &temporary)?;
        if checksum(&temporary)? != expected.to_lowercase() {
            fs::remove_file(&temporary)?;
            return Err(format!("{name} 校验失败，当前模型未更换").into());
        }
        fs::rename(temporary, destination)?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    // Publish only after every file is complete. Existing inference keeps using the old bundle.
    let manifest = root.join(model.directory()).join(MANIFEST);
    let temporary = manifest.with_extension("json.part");
    fs::write(&temporary, serde_json::to_vec_pretty(&bundle)?)?;
    fs::rename(temporary, manifest)?;
    progress(
        bundle.files.len(),
        bundle.files.len(),
        "模型包已校验并保存".into(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Library, Settings, TaskKind};

    #[test]
    fn replacing_a_bundle_is_atomic_and_invalidates_only_its_vectors() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("export");
        fs::create_dir(&source)?;
        for (name, data) in [
            ("model.onnx", "graph"),
            ("model.data", "weights"),
            ("tokenizer.json", "tokenizer"),
        ] {
            fs::write(source.join(name), data)?;
        }
        let mut bundle = Bundle {
            model: EmbeddingModel::Qwen06B,
            onnx: "model.onnx".into(),
            tokenizer: "tokenizer.json".into(),
            output: "last_hidden_state".into(),
            files: ["model.onnx", "model.data", "tokenizer.json"]
                .into_iter()
                .map(|name| Ok((name.into(), checksum(&source.join(name))?)))
                .collect::<Result<_>>()?,
        };
        let manifest = source.join(MANIFEST);
        fs::write(&manifest, serde_json::to_vec(&bundle)?)?;
        let mut library = Library::open(&temp.path().join("library"))?;
        let root = library.root.join("models");
        let cancel = AtomicBool::new(true);
        import(&root, bundle.model, &manifest, &cancel, |_, _, _| {})?;
        assert!(Bundle::installed(&root, bundle.model)?.is_none());
        cancel.store(false, Ordering::Relaxed);
        assert!(import(
            &root,
            EmbeddingModel::Qwen8B,
            &manifest,
            &cancel,
            |_, _, _| {}
        )
        .is_err());
        import(&root, bundle.model, &manifest, &cancel, |_, _, _| {})?;
        let settings = Settings {
            embedding_model: bundle.model,
            auto_index: false,
            ocr_enabled: false,
            ..Default::default()
        };
        library.save_settings(settings.clone())?;
        let image = temp.path().join("meme.png");
        image::RgbaImage::new(4, 4).save(&image)?;
        library.import_files(&[image.to_string_lossy().into_owned()], None);
        let asset = library.assets()?.remove(0);
        library.put_embedding(&asset.id, &Library::search_text(&asset), &[1.0, 0.0])?;
        let previous = bundle.fingerprint()?;

        fs::write(source.join("model.onnx"), "new graph")?;
        bundle
            .files
            .insert("model.onnx".into(), checksum(&source.join("model.onnx"))?);
        fs::write(&manifest, serde_json::to_vec(&bundle)?)?;
        fs::write(source.join("model.data"), "broken")?;
        assert!(import(&root, bundle.model, &manifest, &cancel, |_, _, _| {}).is_err());
        assert_eq!(
            Bundle::installed(&root, bundle.model)?
                .unwrap()
                .fingerprint()?,
            previous
        );
        library.save_settings(settings.clone())?;
        assert_eq!(library.task_counts(TaskKind::Index)?.2, 1);
        fs::write(source.join("model.data"), "weights")?;
        import(&root, bundle.model, &manifest, &cancel, |_, _, _| {})?;
        library.save_settings(settings)?;
        assert_eq!(library.task_counts(TaskKind::Index)?, (1, 0, 0, 1));
        assert!(library
            .search("meme", "semantic", None, Some(&[1.0, 0.0]))?
            .is_empty());
        assert!(Path::new(&asset.path).is_file());

        bundle.files.insert("../escape".into(), "a".repeat(64));
        fs::write(&manifest, serde_json::to_vec(&bundle)?)?;
        assert!(import(&root, bundle.model, &manifest, &cancel, |_, _, _| {}).is_err());
        Ok(())
    }
}
