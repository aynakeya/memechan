use crate::{media, Result};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};
use tempfile::NamedTempFile;

pub struct PreparedImport {
    pub id: String,
    pub name: String,
    pub format: &'static str,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
    pub animated: bool,
    pub original: NamedTempFile,
    pub thumbnail: NamedTempFile,
}

pub fn read_source(path: &Path) -> Result<(String, Vec<u8>)> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err("请拖入图片文件，暂不支持文件夹".into());
    }
    if metadata.len() > 100 * 1024 * 1024 {
        return Err("单张图片不能超过 100 MB".into());
    }
    let bytes = fs::read(path)?;
    let id = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((id, bytes))
}

impl PreparedImport {
    pub fn new(root: &Path, path: &Path, id: String, bytes: &[u8]) -> Result<Self> {
        let info = media::inspect(bytes)?;
        // Stage on the destination filesystem; publishing only needs renames under the lock.
        let mut original = NamedTempFile::new_in(root.join("originals"))?;
        original.write_all(bytes)?;
        let mut thumbnail = NamedTempFile::new_in(root.join("thumbnails"))?;
        info.preview
            .thumbnail(384, 384)
            .write_to(thumbnail.as_file_mut(), image::ImageFormat::Png)?;
        Ok(Self {
            id,
            name: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            format: info.extension,
            width: info.preview.width(),
            height: info.preview.height(),
            bytes: bytes.len(),
            animated: info.animated,
            original,
            thumbnail,
        })
    }
}
