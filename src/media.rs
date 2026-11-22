use std::io::Cursor;
use std::path::Path;
use std::process::{Command, Stdio};

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, RgbaImage};

pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    image.save(path).map_err(|e| e.to_string())
}

pub fn jpeg_bytes(image: &RgbaImage, quality: u8, max_edge: Option<u32>) -> Result<Vec<u8>, String> {
    let rgb = if let Some(edge) = max_edge {
        let (w, h) = fit_edge(image.width(), image.height(), edge);
        let resized = image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle);
        image::DynamicImage::ImageRgba8(resized).to_rgb8()
    } else {
        image::DynamicImage::ImageRgba8(image.clone()).to_rgb8()
    };
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    encoder
        .encode(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

pub fn jpeg_base64(image: &RgbaImage, quality: u8, max_edge: Option<u32>) -> Result<String, String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(jpeg_bytes(image, quality, max_edge)?))
}

pub fn write_thumb(image: &RgbaImage, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, jpeg_bytes(image, 76, Some(480))?).map_err(|e| e.to_string())
}

pub fn read_base64(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub fn copy_image(image: &RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.as_raw()),
        })
        .map_err(|e| e.to_string())
}

pub fn trim_bounds(duration_ms: u64, start_ms: u64, end_ms: u64) -> Option<(u64, u64)> {
    if duration_ms < 400 || end_ms <= start_ms.saturating_add(200) || start_ms >= duration_ms {
        return None;
    }
    Some((start_ms, end_ms.min(duration_ms)))
}

pub fn trim_copy(ffmpeg: &Path, input: &Path, output: &Path, start_ms: u64, end_ms: u64, caption: &str) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let start = format!("{:.3}", start_ms as f64 / 1000.0);
    let end = format!("{:.3}", end_ms as f64 / 1000.0);
    let mut command = Command::new(ffmpeg);
    command.args(["-y", "-i"]).arg(input).args(["-ss", &start, "-to", &end]);
    match caption_filter(caption) {
        Ok(Some(filter)) => {
            command.args(["-vf", &filter, "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-c:a", "copy"]);
        }
        Ok(None) => {
            command.args(["-c", "copy", "-avoid_negative_ts", "make_zero"]);
        }
        Err(()) => return Err("font".into()),
    }
    let status = command
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() && output.is_file() {
        Ok(())
    } else {
        Err("trim".into())
    }
}

pub fn video_thumb(ffmpeg: &Path, video: &Path, thumb: &Path) -> bool {
    if let Some(parent) = thumb.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    Command::new(ffmpeg)
        .args(["-y", "-ss", "0.2", "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-vf", "scale=480:-2"])
        .arg(thumb)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success() && thumb.is_file())
        .unwrap_or(false)
}

fn caption_filter(caption: &str) -> Result<Option<String>, ()> {
    let caption = caption.trim();
    if caption.is_empty() {
        return Ok(None);
    }
    let font = crate::annotate::font_path().ok_or(())?;
    let font = font.display().to_string().replace('\\', "/").replace(':', "\\:");
    let text = caption.replace('\\', "").replace(':', " ").replace('\'', "");
    Ok(Some(format!(
        "drawtext=fontfile='{font}':text='{text}':fontsize=48:fontcolor=white:borderw=3:bordercolor=black:x=48:y=h-th-48"
    )))
}

fn fit_edge(w: u32, h: u32, edge: u32) -> (u32, u32) {
    if w.max(h) <= edge || w == 0 || h == 0 {
        return (w.max(1), h.max(1));
    }
    if w >= h {
        let nw = edge;
        let nh = ((h as u64 * edge as u64) / w as u64).max(1) as u32;
        (nw, nh)
    } else {
        let nh = edge;
        let nw = ((w as u64 * edge as u64) / h as u64).max(1) as u32;
        (nw, nh)
    }
}

pub fn tail(text: &str) -> String {
    let trimmed = text.trim();
    let start = trimmed.len().saturating_sub(420);
    trimmed[start..].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_roundtrip_of_a_solid_frame() {
        let image = RgbaImage::from_pixel(32, 20, image::Rgba([20, 40, 80, 255]));
        let bytes = jpeg_bytes(&image, 80, Some(16)).unwrap();
        assert!(bytes.len() > 100);
        assert!(jpeg_base64(&image, 70, None).unwrap().len() > 100);
    }

    #[test]
    fn trim_with_a_caption_writes_a_new_film() {
        let Some(ffmpeg) = crate::encode::find_tool("ffmpeg") else { return };
        if crate::annotate::font_path().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kapture-trim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("in.mp4");
        let output = dir.join("out.mp4");
        let made = Command::new(&ffmpeg)
            .args(["-y", "-f", "lavfi", "-i", "color=c=black:s=320x180:d=1", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&source)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !made {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        trim_copy(&ffmpeg, &source, &output, 0, 800, "Kapture").unwrap();
        assert!(output.metadata().unwrap().len() > 400);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trim_rejects_a_slice_that_is_too_short() {
        assert_eq!(trim_bounds(10_000, 1_000, 4_000), Some((1_000, 4_000)));
        assert!(trim_bounds(10_000, 4_000, 4_100).is_none());
        assert!(trim_bounds(100, 0, 100).is_none());
    }
}
