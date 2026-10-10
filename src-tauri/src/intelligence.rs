use crate::{media, Result};
use ort::{session::Session, value::Tensor};
use rapid_ocr_rs::{EngineConfig, OcrCallOptions, OcrInput, OcrVersion, RapidOcr, RuntimeConfig};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tokenizers::{Tokenizer, TruncationParams};

mod bundle;
use bundle::Bundle;

const MAX_MODEL_BYTES: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddingModel {
    #[default]
    #[serde(rename = "qwen3-0.6b")]
    Qwen06B,
    #[serde(rename = "qwen3-4b")]
    Qwen4B,
    #[serde(rename = "qwen3-8b")]
    Qwen8B,
}
impl EmbeddingModel {
    pub fn dimensions(self) -> usize {
        match self {
            Self::Qwen06B => 1024,
            Self::Qwen4B => 2560,
            Self::Qwen8B => 4096,
        }
    }
    pub fn version(self) -> &'static str {
        match self {
            Self::Qwen06B => "qwen3-embedding-0.6b-local-last-meme-v1",
            Self::Qwen4B => "qwen3-embedding-4b-local-last-meme-v1",
            Self::Qwen8B => "qwen3-embedding-8b-local-last-meme-v1",
        }
    }
    pub fn directory(self) -> &'static str {
        match self {
            Self::Qwen06B => "qwen3-embedding-0.6b",
            Self::Qwen4B => "qwen3-embedding-4b",
            Self::Qwen8B => "qwen3-embedding-8b",
        }
    }
}

pub struct ModelFile {
    pub name: &'static str,
    sha256: &'static str,
}
pub const OCR_FILES: &[ModelFile] = &[
    ModelFile {
        name: "ocr-v5-det.onnx",
        sha256: "4d97c44a20d30a81aad087d6a396b08f786c4635742afc391f6621f5c6ae78ae",
    },
    ModelFile {
        name: "ocr-cls.onnx",
        sha256: "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c",
    },
    ModelFile {
        name: "ocr-v5-rec.onnx",
        sha256: "5825fc7ebf84ae7a412be049820b4d86d77620f204a041697b0494669b1742c5",
    },
];
pub fn missing(directory: &Path, model: EmbeddingModel) -> Vec<String> {
    let mut files = missing_ocr(directory);
    files.extend(missing_embedding(directory, model));
    files
}
pub fn missing_ocr(directory: &Path) -> Vec<String> {
    missing_files(directory, OCR_FILES)
}
pub fn missing_embedding(directory: &Path, model: EmbeddingModel) -> Vec<String> {
    let result = (|| -> Result<Vec<String>> {
        if let Some(bundle) = Bundle::installed(directory, model)? {
            let path = bundle.directory(directory)?;
            Ok(bundle
                .files
                .keys()
                .filter(|name| !path.join(name).is_file())
                .cloned()
                .collect())
        } else {
            Ok(vec![format!("{}/{}", model.directory(), bundle::MANIFEST)])
        }
    })();
    result.unwrap_or_else(|error| vec![format!("模型清单无效：{error}")])
}
pub fn model_version(directory: &Path, model: EmbeddingModel) -> Result<String> {
    Ok(match Bundle::installed(directory, model)? {
        Some(bundle) => format!("{}-{}", model.version(), bundle.fingerprint()?),
        None => model.version().to_string(),
    })
}
fn missing_files(directory: &Path, files: &[ModelFile]) -> Vec<String> {
    files
        .iter()
        .filter(|file| !directory.join(file.name).is_file())
        .map(|file| file.name.to_string())
        .collect()
}
fn checksum(path: &Path) -> Result<String> {
    let mut reader = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn verify(path: &Path, file: &ModelFile) -> Result<()> {
    if checksum(path)? != file.sha256 {
        return Err(format!("{} 校验失败，请重新导入匹配的文件", file.name).into());
    }
    Ok(())
}

pub fn import_local(
    directory: &Path,
    model: EmbeddingModel,
    paths: &[PathBuf],
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize, String),
) -> Result<()> {
    if let Some(manifest) = paths.iter().find(|path| {
        path.file_name()
            .is_some_and(|name| name == bundle::MANIFEST)
    }) {
        if paths.len() != 1 {
            return Err("导入模型包只需选择 memechan-model.json，关联文件会一并导入".into());
        }
        return bundle::import(directory, model, manifest, cancel, progress);
    }
    let mut selected = Vec::new();
    // Validate the whole selection before touching installed models. Identify by
    // content so files from the original repositories do not need renaming.
    for (index, path) in paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() || metadata.len() > MAX_MODEL_BYTES {
            return Err(format!("{} 不是支持的模型文件", path.display()).into());
        }
        progress(index, paths.len(), format!("校验 {}", path.display()));
        let digest = checksum(path)?;
        let file = OCR_FILES
            .iter()
            .find(|file| file.sha256 == digest)
            .ok_or_else(|| {
                format!(
                    "{} 不匹配；Qwen 模型请选择 memechan-model.json，OCR 请使用 README 列出的文件",
                    path.display()
                )
            })?;
        selected.push((path, file));
    }
    fs::create_dir_all(directory)?;
    for (index, (path, file)) in selected.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let target = directory.join(file.name);
        fs::create_dir_all(target.parent().unwrap())?;
        if !target.is_file() || checksum(&target)? != file.sha256 {
            progress(index, selected.len(), format!("复制 {}", file.name));
            let temporary = target.with_file_name(format!(
                "{}.part",
                target.file_name().unwrap().to_string_lossy()
            ));
            fs::copy(path, &temporary)?;
            verify(&temporary, file)?;
            fs::rename(temporary, target)?;
        }
        progress(
            index + 1,
            selected.len(),
            format!("{} 已导入并校验", file.name),
        );
    }
    Ok(())
}
pub struct Intelligence {
    directory: PathBuf,
    pub model: EmbeddingModel,
    pub version: String,
    ocr: Option<RapidOcr>,
    embedding: Option<Embedding>,
}
struct Embedding {
    session: Session,
    tokenizer: Tokenizer,
    output: String,
}
impl Intelligence {
    pub fn load(directory: &Path, model: EmbeddingModel) -> Result<Self> {
        ort::init().with_telemetry(false).commit();
        Ok(Self {
            directory: directory.to_path_buf(),
            model,
            version: model_version(directory, model)?,
            ocr: None,
            embedding: None,
        })
    }
    pub fn ensure_ocr(&mut self) -> Result<()> {
        if self.ocr.is_some() {
            return Ok(());
        }
        let directory = &self.directory;
        for file in OCR_FILES {
            verify(&directory.join(file.name), file)?;
        }
        let runtime = RuntimeConfig {
            intra_threads: Some(2),
            inter_threads: Some(1),
            rayon_threads: Some(2),
            ..Default::default()
        };
        let mut config = EngineConfig::default();
        config.det.model_path = Some(directory.join("ocr-v5-det.onnx"));
        config.det.ocr_version = OcrVersion::PPocrV5;
        config.det.allow_download = false;
        config.det.limit_side_len = 960;
        config.det.limit_type = "max".into();
        config.det.runtime = runtime.clone();
        config.cls.model_path = Some(directory.join("ocr-cls.onnx"));
        config.cls.allow_download = false;
        config.cls.runtime = runtime.clone();
        config.rec.model.model_path = Some(directory.join("ocr-v5-rec.onnx"));
        config.rec.model.ocr_version = OcrVersion::PPocrV5;
        config.rec.model.allow_download = false;
        config.rec.runtime = runtime;
        self.ocr = Some(RapidOcr::new(config)?);
        Ok(())
    }
    pub fn ensure_embedding(&mut self) -> Result<()> {
        if self.embedding.is_some() {
            return Ok(());
        }
        let bundle = Bundle::installed(&self.directory, self.model)?
            .ok_or("请先导入所选 Qwen 的 memechan-model.json")?;
        let directory = bundle.directory(&self.directory)?;
        bundle.verify(&directory)?;
        let output = bundle.output;
        let session = Session::builder()?
            .with_intra_threads(2)
            .map_err(ort::Error::<()>::from)?
            .with_inter_threads(1)
            .map_err(ort::Error::<()>::from)?
            .commit_from_file(directory.join(bundle.onnx))?;
        if !session.outputs().iter().any(|item| item.name() == output) {
            return Err(format!("ONNX 缺少输出 {output}").into());
        }
        let mut tokenizer = Tokenizer::from_file(directory.join(bundle.tokenizer))?;
        tokenizer.with_padding(None);
        tokenizer.with_truncation(Some(TruncationParams {
            max_length: 512,
            ..Default::default()
        }))?;
        self.embedding = Some(Embedding {
            session,
            tokenizer,
            output,
        });
        Ok(())
    }
    pub fn recognize(&mut self, path: &Path) -> Result<String> {
        self.ensure_ocr()?;
        let mut texts = Vec::new();
        for frame in media::sample_frames(path)? {
            // White compositing avoids turning transparent lettering into a black background.
            let rgb = media::rgb_on_white(&frame);
            let result = self.ocr.as_mut().unwrap().run(
                OcrInput::Rgb {
                    width: rgb.width() as usize,
                    height: rgb.height() as usize,
                    data: rgb.into_raw(),
                },
                OcrCallOptions::default(),
            )?;
            for text in result.txts.unwrap_or_default() {
                let text = text.trim().to_string();
                if !text.is_empty() && !texts.contains(&text) {
                    texts.push(text);
                }
            }
        }
        Ok(texts.join("\n"))
    }
    pub fn embed(&mut self, text: &str, query: bool) -> Result<Vec<f32>> {
        self.ensure_embedding()?;
        let dimensions = self.model.dimensions();
        let model = self.embedding.as_mut().unwrap();
        let text = if query {
            format!("Instruct: Given a description of a reaction, emotion, or action, retrieve the most relevant meme.\nQuery: {text}")
        } else {
            text.to_string()
        };
        let encoding = model.tokenizer.encode(text, true)?;
        let length = encoding.len();
        let mut inputs = Vec::new();
        for input in model.session.inputs() {
            // Each text is independent; the exported decoder starts with an empty cache.
            if input.name().starts_with("past_key_values.") {
                let shape = input.dtype().tensor_shape().ok_or("无效的缓存输入")?;
                if shape.len() != 4 || shape[1] <= 0 || shape[3] <= 0 {
                    return Err("不支持的模型缓存形状".into());
                }
                inputs.push((
                    input.name().to_owned(),
                    Tensor::<f32>::new(model.session.allocator(), [1, shape[1], 0, shape[3]])?
                        .into_dyn(),
                ));
                continue;
            }
            let values: Vec<i64> = match input.name() {
                "input_ids" => encoding.get_ids().iter().map(|&id| i64::from(id)).collect(),
                "attention_mask" => encoding
                    .get_attention_mask()
                    .iter()
                    .map(|&mask| i64::from(mask))
                    .collect(),
                "position_ids" => (0..length as i64).collect(),
                name => return Err(format!("不支持的模型输入：{name}").into()),
            };
            inputs.push((
                input.name().to_owned(),
                Tensor::from_array(([1, length], values))?.into_dyn(),
            ));
        }
        let outputs = model.session.run(inputs)?;
        let (shape, values) = outputs[model.output.as_str()].try_extract_tensor::<f32>()?;
        if shape.len() != 3
            || shape[0] != 1
            || shape[1] != length as i64
            || shape[2] != dimensions as i64
        {
            return Err("语义模型输出维度不匹配".into());
        }
        let mut vector = values[(length - 1) * dimensions..length * dimensions].to_vec();
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if !norm.is_finite() || norm == 0.0 {
            return Err("语义模型返回了无效向量".into());
        }
        for value in &mut vector {
            *value /= norm;
        }
        Ok(vector)
    }
}
