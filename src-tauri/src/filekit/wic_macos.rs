use crate::error::{AppError, AppResult};
use image::ImageEncoder;
use std::path::Path;
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
pub fn shrink(img: &Rgba, max: u32) -> Rgba {
    let k = max as f64 / img.width.max(img.height) as f64;
    let (w, h) = (
        ((img.width as f64 * k).round() as u32).max(1),
        ((img.height as f64 * k).round() as u32).max(1),
    );
    let Some(buf) = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone()) else {
        return Rgba {
            width: img.width,
            height: img.height,
            pixels: img.pixels.clone(),
        };
    };
    let out = image::imageops::resize(&buf, w, h, image::imageops::FilterType::Triangle);
    Rgba {
        width: w,
        height: h,
        pixels: out.into_raw(),
    }
}
pub fn encode_png(img: &Rgba) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            &img.pixels,
            img.width,
            img.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| AppError::internal(format!("PNG: {e}")))?;
    Ok(out)
}
pub fn encode_webp_lossless(img: &Rgba) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .write_image(
            &img.pixels,
            img.width,
            img.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| AppError::internal(format!("WebP: {e}")))?;
    Ok(out)
}
pub fn flatten_rgb(img: &Rgba) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(img.pixels.len() / 4 * 3);
    for p in img.pixels.chunks_exact(4) {
        let a = p[3] as u32;
        for c in &p[..3] {
            rgb.push(((*c as u32 * a + 255 * (255 - a)) / 255) as u8);
        }
    }
    rgb
}
pub fn encode_jpeg(img: &Rgba, quality: u8) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .write_image(
            &flatten_rgb(img),
            img.width,
            img.height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| AppError::internal(format!("JPEG: {e}")))?;
    Ok(out)
}
pub fn thumbnail_data_url(img: &Rgba) -> Option<String> {
    use base64::Engine;
    let png = encode_png(img).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}
pub fn decode_png_bytes(png: &[u8]) -> AppResult<Rgba> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| AppError::internal(format!("PNG: {e}")))?;
    let rgba = img.to_rgba8();
    Ok(Rgba {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}
pub fn decode(path: &Path, max_side: Option<u32>) -> AppResult<Rgba> {
    let (width, height) = dimensions(path)?;
    if u64::from(width) * u64::from(height) > 80_000_000 {
        return Err(AppError::unsupported(
            "Изображение слишком большое для обработки",
        ));
    }
    let image = image::open(path)
        .map_err(|e| AppError::invalid(format!("Не удалось открыть изображение: {e}")))?
        .to_rgba8();
    let decoded = Rgba {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    };
    Ok(match max_side.filter(|m| width.max(height) > *m) {
        Some(max) => shrink(&decoded, max),
        None => decoded,
    })
}
pub fn dimensions(path: &Path) -> AppResult<(u32, u32)> {
    image::image_dimensions(path)
        .map_err(|e| AppError::invalid(format!("Не удалось прочитать изображение: {e}")))
}
pub struct Pdf;
impl Pdf {
    pub fn open(_path: &Path) -> AppResult<Self> {
        Err(AppError::unsupported("PDF пока не поддерживается на macOS"))
    }
    pub fn pages(&self) -> u32 {
        0
    }
    pub fn render_png(&self, _index: u32, _width: u32) -> AppResult<Vec<u8>> {
        Err(AppError::unsupported("PDF пока не поддерживается на macOS"))
    }
    pub fn page_width_pt(&self, _index: u32) -> f32 {
        0.0
    }
}
pub fn ocr_image(_path: &Path) -> AppResult<String> {
    Err(AppError::unsupported("OCR пока не поддерживается на macOS"))
}
pub fn ocr_pdf(_path: &Path, _progress: impl FnMut(u32, u32) -> bool) -> AppResult<String> {
    Err(AppError::unsupported("OCR пока не поддерживается на macOS"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_file_preserves_pixels_and_limits_size() {
        let path = std::env::temp_dir().join(format!("jarvis-image-{}.png", uuid::Uuid::new_v4()));
        let image = image::RgbaImage::from_pixel(8, 4, image::Rgba([20, 40, 60, 255]));
        image.save(&path).unwrap();
        assert_eq!(dimensions(&path).unwrap(), (8, 4));
        let decoded = decode(&path, None).unwrap();
        assert_eq!(decoded.pixels, image.into_raw());
        let thumbnail = decode(&path, Some(4)).unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (4, 2));
        std::fs::write(&path, []).unwrap();
        assert!(decode(&path, None).is_err());
        std::fs::write(&path, b"not an image").unwrap();
        assert!(dimensions(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(decode(&path, None).is_err());
    }
}
