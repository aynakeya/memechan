use crate::Result;
use image::{
    codecs::{
        gif::{GifDecoder, GifEncoder, Repeat},
        png::PngDecoder,
        webp::WebPDecoder,
    },
    imageops::FilterType,
    metadata::{LoopCount, Orientation},
    AnimationDecoder, DynamicImage, Frame, ImageDecoder, ImageFormat, ImageReader,
};
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Cursor, Write},
    path::{Path, PathBuf},
};

pub struct ImageInfo {
    pub extension: &'static str,
    pub animated: bool,
    pub preview: DynamicImage,
}
pub fn inspect(bytes: &[u8]) -> Result<ImageInfo> {
    let format = image::guess_format(bytes)?;
    let extension = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        _ => return Err("仅支持 PNG、JPG、GIF、WebP 图片".into()),
    };
    let animated = match format {
        ImageFormat::Gif => true,
        ImageFormat::Png => PngDecoder::new(Cursor::new(bytes))?.is_apng()?,
        ImageFormat::WebP => WebPDecoder::new(Cursor::new(bytes))?.has_animation(),
        _ => false,
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut preview = DynamicImage::from_decoder(decoder)?;
    preview.apply_orientation(orientation);
    Ok(ImageInfo {
        extension,
        animated,
        preview,
    })
}
pub fn dimensions(width: u32, height: u32, size: u32, upscale: bool) -> (u32, u32) {
    if size == 0 || (!upscale && width.max(height) <= size) {
        return (width, height);
    }
    let scale = size as f64 / width.max(height) as f64;
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}
pub fn prepare_copy(
    asset: &crate::library::Asset,
    root: &Path,
    size: u32,
    upscale: bool,
) -> Result<PathBuf> {
    if size > 4096 {
        return Err("复制最长边不能超过 4096 px".into());
    }
    let (width, height) = dimensions(asset.width, asset.height, size, upscale);
    let original = size == 0 || (width == asset.width && height == asset.height);
    let extension = if original {
        asset.format.as_str()
    } else if asset.animated {
        "gif"
    } else {
        "png"
    };
    let output = root
        .join("copies")
        .join(format!("{}-{}x{}-v1.{extension}", asset.id, width, height));
    if output.exists() {
        File::options()
            .write(true)
            .open(&output)?
            .set_modified(std::time::SystemTime::now())?;
        return Ok(output);
    }
    let temporary = output.with_extension("part");
    let result = (|| -> Result<()> {
        if original {
            fs::copy(&asset.path, &temporary)?;
        } else if asset.animated {
            let reader = BufReader::new(File::open(&asset.path)?);
            match asset.format.as_str() {
                "gif" => resize_gif(Path::new(&asset.path), &temporary, width, height)?,
                "webp" => {
                    let decoder = WebPDecoder::new(reader)?;
                    let repeat = animation_repeat(decoder.loop_count());
                    resize_animation(decoder, &temporary, width, height, repeat)?;
                }
                "png" => {
                    let decoder = PngDecoder::new(reader)?.apng()?;
                    let repeat = animation_repeat(decoder.loop_count());
                    resize_animation(decoder, &temporary, width, height, repeat)?;
                }
                _ => return Err("此动图格式暂不支持缩放".into()),
            }
        } else {
            let image = inspect(&fs::read(&asset.path)?)?.preview;
            image
                .resize_exact(width, height, FilterType::Lanczos3)
                .save_with_format(&temporary, ImageFormat::Png)?;
        }
        fs::rename(&temporary, &output)?;
        Ok(())
    })();
    if result.is_err() && temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    result?;
    Ok(output)
}

pub fn clipboard_png(path: &Path) -> Result<Vec<u8>> {
    // prepare_copy names immutable artifacts by content hash and output dimensions.
    // These PNG variants share the same copies/ capacity limit and cleanup lock.
    let cached = path.with_extension("clipboard-v1.png");
    if cached.exists() {
        let bytes = fs::read(&cached)?;
        File::options()
            .write(true)
            .open(&cached)?
            .set_modified(std::time::SystemTime::now())?;
        return Ok(bytes);
    }
    let bytes = fs::read(path)?;
    if image::guess_format(&bytes)? == ImageFormat::Png
        && PngDecoder::new(Cursor::new(&bytes))?.orientation()? == Orientation::NoTransforms
    {
        // The native PNG clipboard format can use the original bytes without a round trip.
        return Ok(bytes);
    }
    let image = inspect(&bytes)?.preview;
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, ImageFormat::Png)?;
    let png = png.into_inner();
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(&png)?;
    temporary.persist(cached)?;
    Ok(png)
}

fn animation_repeat(count: LoopCount) -> Option<Repeat> {
    match count {
        LoopCount::Infinite => Some(Repeat::Infinite),
        LoopCount::Finite(n) if n.get() == 1 => None,
        LoopCount::Finite(n) => Some(Repeat::Finite((n.get() - 1).min(u16::MAX as u32) as u16)),
    }
}
fn resize_gif(source: &Path, output: &Path, width: u32, height: u32) -> Result<()> {
    // Read the GIF repeat extension directly: image's generic loop_count maps
    // a missing extension to infinity, losing the distinction of playing once.
    let metadata = gif::DecodeOptions::new().read_info(BufReader::new(File::open(source)?))?;
    let repeat = match metadata.repeat() {
        gif::Repeat::Finite(0) => None,
        gif::Repeat::Finite(n) => Some(Repeat::Finite(n)),
        gif::Repeat::Infinite => Some(Repeat::Infinite),
    };
    resize_animation(
        GifDecoder::new(BufReader::new(File::open(source)?))?,
        output,
        width,
        height,
        repeat,
    )
}
fn resize_animation<'a>(
    decoder: impl AnimationDecoder<'a>,
    output: &Path,
    width: u32,
    height: u32,
    repeat: Option<Repeat>,
) -> Result<()> {
    let mut writer = BufWriter::new(File::create(output)?);
    let mut encoder = GifEncoder::new_with_speed(&mut writer, 10);
    if let Some(repeat) = repeat {
        encoder.set_repeat(repeat)?;
    }
    for (index, frame) in decoder.into_frames().enumerate() {
        if index >= 3000 {
            return Err("动图超过 3000 帧，请按原尺寸复制".into());
        }
        let frame = frame?;
        let delay = frame.delay();
        let buffer = image::imageops::resize(frame.buffer(), width, height, FilterType::Lanczos3);
        encoder.encode_frame(Frame::from_parts(buffer, 0, 0, delay))?;
    }
    drop(encoder);
    writer.flush()?;
    Ok(())
}
pub fn rgb_on_white(image: &DynamicImage) -> image::RgbImage {
    let mut background =
        image::RgbImage::from_pixel(image.width(), image.height(), image::Rgb([255; 3]));
    for (target, source) in background.pixels_mut().zip(image.to_rgba8().pixels()) {
        let alpha = u32::from(source[3]);
        for channel in 0..3 {
            target[channel] =
                ((u32::from(source[channel]) * alpha + 255 * (255 - alpha)) / 255) as u8;
        }
    }
    background
}

pub fn sample_frames(path: &Path) -> Result<Vec<DynamicImage>> {
    let bytes = fs::read(path)?;
    let info = inspect(&bytes)?;
    if !info.animated {
        return Ok(vec![info.preview]);
    }
    let reader = Cursor::new(bytes);
    let frames = match info.extension {
        "gif" => GifDecoder::new(reader)?.into_frames(),
        "webp" => WebPDecoder::new(reader)?.into_frames(),
        "png" => PngDecoder::new(reader)?.apng()?.into_frames(),
        _ => return Err("不支持的动图格式".into()),
    };
    // Bounded sampling keeps GIF analysis from loading every decoded frame into RAM.
    let mut samples = vec![];
    let mut last = None;
    for (index, frame) in frames.take(180).enumerate() {
        let frame = frame?;
        if [0, 15, 45, 90].contains(&index) {
            samples.push(DynamicImage::ImageRgba8(frame.into_buffer()));
            last = None;
        } else {
            last = Some(DynamicImage::ImageRgba8(frame.into_buffer()));
        }
    }
    if let Some(last) = last {
        samples.push(last);
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_png_preserves_pixels_alpha_and_display_orientation() -> Result<()> {
        use image::{codecs::png::PngEncoder, ImageEncoder};
        let dir = tempfile::tempdir()?;
        let image = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([x as u8 * 80, y as u8 * 120, 64, x as u8 * 100])
        }));
        for rotate in [false, true] {
            let path = dir.path().join(format!("source-{rotate}.png"));
            let mut bytes = Vec::new();
            let mut encoder = PngEncoder::new(&mut bytes);
            if rotate {
                // TIFF IFD with EXIF orientation 6 (90 degrees clockwise).
                encoder.set_exif_metadata(vec![
                    b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0,
                    0, 0, 0,
                ])?;
            }
            image.write_with_encoder(encoder)?;
            fs::write(&path, &bytes)?;
            let copied = image::load_from_memory(&clipboard_png(&path)?)?.to_rgba8();
            let expected = if rotate {
                image.rotate90()
            } else {
                image.clone()
            };
            assert_eq!(copied, expected.to_rgba8());
            assert_eq!(fs::read(&path)?, bytes);
            assert_eq!(path.with_extension("clipboard-v1.png").exists(), rotate);
        }
        Ok(())
    }
    #[test]
    fn sizes_preserve_aspect_and_small_images() {
        assert_eq!(dimensions(800, 600, 256, false), (256, 192));
        assert_eq!(dimensions(100, 60, 256, false), (100, 60));
        assert_eq!(dimensions(100, 60, 256, true), (256, 154));
    }
    #[test]
    fn resizing_gif_preserves_frames_delays_and_looping() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source.gif");
        for repeat in [None, Some(Repeat::Finite(3)), Some(Repeat::Infinite)] {
            {
                let mut encoder = GifEncoder::new(File::create(&source)?);
                if let Some(repeat) = repeat {
                    encoder.set_repeat(repeat)?;
                }
                for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                    encoder.encode_frame(Frame::from_parts(
                        image::RgbaImage::from_pixel(40, 20, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(120, 1),
                    ))?;
                }
            }
            let output = dir.path().join("resized.gif");
            resize_gif(&source, &output, 20, 10)?;
            let source_repeat = gif::DecodeOptions::new()
                .read_info(BufReader::new(File::open(&source)?))?
                .repeat();
            let output_repeat = gif::DecodeOptions::new()
                .read_info(BufReader::new(File::open(&output)?))?
                .repeat();
            assert_eq!(source_repeat, output_repeat);
            let decoded = GifDecoder::new(BufReader::new(File::open(output)?))?;
            let frames = decoded.into_frames().collect_frames()?;
            assert_eq!(frames.len(), 2);
            assert_eq!(frames[0].buffer().dimensions(), (20, 10));
            assert_eq!(frames[0].delay().numer_denom_ms(), (120, 1));
            assert_ne!(frames[0].buffer(), frames[1].buffer());
        }
        Ok(())
    }
}
