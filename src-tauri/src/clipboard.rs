use crate::{
    library::{Asset, Settings},
    media, Result,
};
#[cfg(target_os = "windows")]
use clipboard_rs::{common::RustImage, RustImageData};
use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext, ContentFormat};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
#[derive(Default)]
struct ClipboardSession {
    context: Option<ClipboardContext>,
    last_copy: Option<PathBuf>,
}
#[derive(Default)]
pub struct ClipboardState(Mutex<ClipboardSession>);
impl ClipboardState {
    pub fn referenced_files(&self) -> Result<Vec<PathBuf>> {
        let mut clipboard = self.0.lock().unwrap();
        if clipboard.context.is_none() {
            clipboard.context = Some(ClipboardContext::new()?);
        }
        let context = clipboard.context.as_ref().unwrap();
        // has() alone masks read errors on some platforms.
        let formats = context.available_formats()?;
        let mut paths: Vec<PathBuf> = clipboard.last_copy.iter().cloned().collect();
        if formats.iter().any(|format| format == "text/uri-list")
            || context.has(ContentFormat::Files)
        {
            let files = context.get_files()?;
            if files.is_empty() {
                return Err("无法读取当前剪贴板文件".into());
            }
            for file in files {
                if let Some(path) = file.strip_prefix("file://") {
                    // clipboard-rs emits raw file URIs on Linux; other apps may escape them.
                    paths.push(PathBuf::from(path));
                    if let Some(path) = tauri::Url::parse(&file)
                        .ok()
                        .and_then(|url| url.to_file_path().ok())
                    {
                        paths.push(path);
                    }
                } else {
                    paths.push(PathBuf::from(file));
                }
            }
        }
        Ok(paths)
    }

    pub fn copy(
        &self,
        asset: &Asset,
        root: &Path,
        settings: &Settings,
        size: Option<u32>,
    ) -> Result<String> {
        // Serialize both cached-file creation and writing to the native clipboard.
        let mut clipboard = self.0.lock().unwrap();
        let size = size.unwrap_or(settings.copy_size);
        let path = media::prepare_copy(asset, root, size, settings.allow_upscale)?;
        if clipboard.context.is_none() {
            clipboard.context = Some(ClipboardContext::new()?);
        }
        let mut contents = vec![ClipboardContent::Files(vec![path
            .to_string_lossy()
            .into_owned()])];
        if asset.animated {
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("gif");
            let format = match extension {
                "gif" => {
                    if cfg!(target_os = "macos") {
                        "com.compuserve.gif"
                    } else if cfg!(target_os = "windows") {
                        "GIF"
                    } else {
                        "image/gif"
                    }
                }
                "webp" => "image/webp",
                _ => {
                    if cfg!(target_os = "macos") {
                        "public.png"
                    } else {
                        "image/png"
                    }
                }
            };
            contents.push(ClipboardContent::Other(
                format.into(),
                std::fs::read(&path)?,
            ));
        } else {
            #[cfg(target_os = "windows")]
            {
                // Windows also needs the native bitmap formats provided by clipboard-rs.
                let preview = media::inspect(&std::fs::read(&path)?)?.preview;
                contents.push(ClipboardContent::Image(RustImageData::from_dynamic_image(
                    preview,
                )));
            }
            #[cfg(not(target_os = "windows"))]
            contents.push(ClipboardContent::Other(
                if cfg!(target_os = "macos") {
                    "public.png"
                } else {
                    "image/png"
                }
                .into(),
                media::clipboard_png(&path)?,
            ));
        }
        clipboard.context.as_ref().unwrap().set(contents)?;
        clipboard.last_copy = Some(path);
        Ok(if asset.animated {
            "已复制动图文件；粘贴效果取决于目标应用的文件粘贴支持".into()
        } else {
            "已复制图片".into()
        })
    }
}
