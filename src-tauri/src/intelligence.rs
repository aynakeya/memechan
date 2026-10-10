use crate::{media, Result};
use ort::{session::Session, value::Tensor};
use paddle_ocr_rs::ocr_lite::OcrLite;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use tokenizers::{Tokenizer, TruncationParams};

pub const MODEL_VERSION: &str = "qwen3-embedding-0.6b-c25a394d-int8-last-meme-v1";
pub const EMBEDDING_DIMENSIONS: usize = 1024;
const MAX_MODEL_BYTES: u64 = 700 * 1024 * 1024;

pub struct ModelFile {
    pub name: &'static str,
    sha256: &'static str,
}
pub const FILES: &[ModelFile] = &[
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
    ModelFile {
        name: "qwen3-embedding-0.6b.onnx",
        sha256: "87cd124e0ef1fd1f223ebc283efccbaeac386d0b08344701c46975d0657b591f",
    },
    ModelFile {
        name: "qwen3-tokenizer.json",
        sha256: "def76fb086971c7867b829c23a26261e38d9d74e02139253b38aeb9df8b4b50a",
    },
];
pub fn missing(directory: &Path) -> Vec<String> {
    missing_files(directory, FILES)
}
pub fn missing_ocr(directory: &Path) -> Vec<String> {
    missing_files(directory, &FILES[..3])
}
pub fn missing_embedding(directory: &Path) -> Vec<String> {
    missing_files(directory, &FILES[3..])
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
    paths: &[PathBuf],
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize, String),
) -> Result<()> {
    let mut selected = Vec::new();
    // Validate the whole selection before touching installed models. Identify by
    // content so files from the original repositories do not need renaming.
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() || metadata.len() > MAX_MODEL_BYTES {
            return Err(format!("{} 不是支持的模型文件", path.display()).into());
        }
        let digest = checksum(path)?;
        let file = FILES.iter().find(|file| file.sha256 == digest).ok_or_else(|| {
            format!("{} 与当前支持的模型不匹配，请选择 README 中列出的 PP-OCRv5、Qwen3-Embedding-0.6B 和 tokenizer 文件", path.display())
        })?;
        selected.push((path, file));
    }
    fs::create_dir_all(directory)?;
    for (index, (path, file)) in selected.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let target = directory.join(file.name);
        if !target.is_file() || checksum(&target)? != file.sha256 {
            let temporary = target.with_extension("part");
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
    ocr: Option<OcrLite>,
    embedding: Option<Embedding>,
}
struct Embedding {
    session: Session,
    tokenizer: Tokenizer,
}
impl Intelligence {
    pub fn load(directory: &Path) -> Result<Self> {
        ort::init().with_telemetry(false).commit()?;
        Ok(Self {
            directory: directory.to_path_buf(),
            ocr: None,
            embedding: None,
        })
    }
    pub fn ensure_ocr(&mut self) -> Result<()> {
        if self.ocr.is_some() {
            return Ok(());
        }
        let directory = &self.directory;
        for file in &FILES[..3] {
            verify(&directory.join(file.name), file)?;
        }
        let mut ocr = OcrLite::new();
        ocr.init_models(
            &directory.join("ocr-v5-det.onnx").to_string_lossy(),
            &directory.join("ocr-cls.onnx").to_string_lossy(),
            &directory.join("ocr-v5-rec.onnx").to_string_lossy(),
            2,
        )?;
        self.ocr = Some(ocr);
        Ok(())
    }
    pub fn ensure_embedding(&mut self) -> Result<()> {
        if self.embedding.is_some() {
            return Ok(());
        }
        let directory = &self.directory;
        for file in &FILES[3..] {
            verify(&directory.join(file.name), file)?;
        }
        let session = Session::builder()?
            .with_intra_threads(2)?
            .with_inter_threads(1)?
            .commit_from_file(directory.join("qwen3-embedding-0.6b.onnx"))?;
        let mut tokenizer = Tokenizer::from_file(directory.join("qwen3-tokenizer.json"))?;
        tokenizer.with_padding(None);
        tokenizer.with_truncation(Some(TruncationParams {
            max_length: 512,
            ..Default::default()
        }))?;
        self.embedding = Some(Embedding { session, tokenizer });
        Ok(())
    }
    pub fn recognize(&mut self, path: &Path) -> Result<String> {
        self.ensure_ocr()?;
        let mut texts = Vec::new();
        for frame in media::sample_frames(path)? {
            // White compositing avoids turning transparent lettering into a black background.
            let rgb = media::rgb_on_white(&frame);
            let result = self
                .ocr
                .as_mut()
                .unwrap()
                .detect(&rgb, 32, 960, 0.5, 0.3, 1.6, true, false)?;
            for block in result.text_blocks {
                let text = block.text.trim().to_string();
                if !text.is_empty() && block.text_score >= 0.5 && !texts.contains(&text) {
                    texts.push(text);
                }
            }
        }
        Ok(texts.join("\n"))
    }
    pub fn embed(&mut self, text: &str, query: bool) -> Result<Vec<f32>> {
        self.ensure_embedding()?;
        let model = self.embedding.as_mut().unwrap();
        let text = if query {
            format!("Instruct: Given a description of a reaction, emotion, or action, retrieve the most relevant meme.\nQuery: {text}")
        } else {
            text.to_string()
        };
        let encoding = model.tokenizer.encode(text, true)?;
        let length = encoding.len();
        let mut inputs = Vec::new();
        for input in &model.session.inputs {
            // Each text is independent; the exported decoder starts with an empty cache.
            if input.name.starts_with("past_key_values.") {
                let shape = input.input_type.tensor_shape().ok_or("无效的缓存输入")?;
                if shape.len() != 4 || shape[1] <= 0 || shape[3] <= 0 {
                    return Err("不支持的模型缓存形状".into());
                }
                inputs.push((
                    input.name.clone(),
                    Tensor::<f32>::new(model.session.allocator(), [1, shape[1], 0, shape[3]])?
                        .into_dyn(),
                ));
                continue;
            }
            let values: Vec<i64> = match input.name.as_str() {
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
                input.name.clone(),
                Tensor::from_array(([1, length], values))?.into_dyn(),
            ));
        }
        let outputs = model.session.run(inputs)?;
        let (shape, values) = outputs["last_hidden_state"].try_extract_tensor::<f32>()?;
        if shape.len() != 3
            || shape[0] != 1
            || shape[1] != length as i64
            || shape[2] != EMBEDDING_DIMENSIONS as i64
        {
            return Err("语义模型输出维度不匹配".into());
        }
        let mut vector =
            values[(length - 1) * EMBEDDING_DIMENSIONS..length * EMBEDDING_DIMENSIONS].to_vec();
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
