use crate::{
    library::{Asset, Settings},
    media, Result,
};
#[cfg(target_os = "windows")]
use clipboard_rs::{common::RustImage, RustImageData};
use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
use std::{path::Path, sync::Mutex};
#[derive(Default)]
pub struct ClipboardState(Mutex<Option<ClipboardContext>>);
impl ClipboardState {
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
        if clipboard.is_none() {
            *clipboard = Some(ClipboardContext::new()?);
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
        clipboard.as_ref().unwrap().set(contents)?;
        Ok(if asset.animated {
            "已复制动图文件；粘贴效果取决于目标应用的文件粘贴支持".into()
        } else {
            "已复制图片".into()
        })
    }
}
