//! Images, PDF pages and text recognition through Windows itself (WinRT):
//! the system decoders read everything the user's Windows can open —
//! JPEG, PNG, GIF, BMP, TIFF, ICO and, with the Store extensions installed
//! on this machine, HEIC/HEIF, WebP and camera RAW. OCR is Windows.Media.Ocr
//! (Russian and English packs), so recognition stays on this computer.

use std::path::Path;

use windows::core::HSTRING;
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode,
    ExifOrientationMode, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataReader, DataWriter, IRandomAccessStream, InMemoryRandomAccessStream};
use windows::Storage::{FileAccessMode, StorageFile};

use crate::error::{AppError, AppResult};

fn platform(what: &str, e: windows::core::Error) -> AppError {
    AppError::platform(format!("{what}: {}", e.message()))
}

/// Absolute Windows path (WinRT refuses relative and forward-slash paths).
fn winrt_path(path: &Path) -> AppResult<HSTRING> {
    let abs = std::path::absolute(path).map_err(|e| AppError::invalid(format!("Путь {}: {e}", path.display())))?;
    Ok(HSTRING::from(abs.to_string_lossy().replace('/', "\\")))
}

fn open_stream(path: &Path) -> AppResult<IRandomAccessStream> {
    let file = StorageFile::GetFileFromPathAsync(&winrt_path(path)?)
        .and_then(|op| op.join())
        .map_err(|e| platform("Не удалось открыть файл", e))?;
    file.OpenAsync(FileAccessMode::Read).and_then(|op| op.join()).map_err(|e| platform("Не удалось прочитать файл", e))
}

/// Decoded straight-alpha RGBA, orientation and colour normalised to sRGB.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

fn decoder(stream: &IRandomAccessStream) -> AppResult<BitmapDecoder> {
    BitmapDecoder::CreateAsync(stream)
        .and_then(|op| op.join())
        .map_err(|e| platform("Windows не умеет открывать такое изображение (нужен кодек)", e))
}

fn decode_with(dec: &BitmapDecoder, max_side: Option<u32>) -> AppResult<Rgba> {
    let (w, h) = (dec.OrientedPixelWidth().unwrap_or(0), dec.OrientedPixelHeight().unwrap_or(0));
    if w == 0 || h == 0 {
        return Err(AppError::invalid("Изображение пустое"));
    }
    let transform = BitmapTransform::new().map_err(|e| platform("WIC", e))?;
    let data = dec
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::ColorManageToSRgb,
        )
        .and_then(|op| op.join())
        .map_err(|e| platform("Не удалось декодировать изображение", e))?;
    let pixels = data.DetachPixelData().map_err(|e| platform("WIC", e))?.to_vec();
    if pixels.len() != (w as usize) * (h as usize) * 4 {
        return Err(AppError::platform("Декодер вернул неожиданный размер изображения"));
    }
    let img = Rgba { width: w, height: h, pixels };
    Ok(match max_side.filter(|m| w.max(h) > *m) {
        Some(max) => shrink(&img, max),
        None => img,
    })
}

/// Downscale so the longer side is `max` (orientation already applied).
pub fn shrink(img: &Rgba, max: u32) -> Rgba {
    let k = max as f64 / img.width.max(img.height) as f64;
    let (w, h) = (((img.width as f64 * k).round() as u32).max(1), ((img.height as f64 * k).round() as u32).max(1));
    let Some(buf) = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone()) else {
        return Rgba { width: img.width, height: img.height, pixels: img.pixels.clone() };
    };
    let out = image::imageops::resize(&buf, w, h, image::imageops::FilterType::Triangle);
    Rgba { width: w, height: h, pixels: out.into_raw() }
}

pub fn decode(path: &Path, max_side: Option<u32>) -> AppResult<Rgba> {
    let stream = open_stream(path)?;
    decode_with(&decoder(&stream)?, max_side)
}

/// Width and height after orientation, without decoding pixels.
pub fn dimensions(path: &Path) -> AppResult<(u32, u32)> {
    let stream = open_stream(path)?;
    let dec = decoder(&stream)?;
    Ok((dec.OrientedPixelWidth().unwrap_or(0), dec.OrientedPixelHeight().unwrap_or(0)))
}

pub fn encode_png(img: &Rgba) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&img.pixels, img.width, img.height, image::ExtendedColorType::Rgba8)
        .map_err(|e| AppError::internal(format!("PNG: {e}")))?;
    Ok(out)
}

use image::ImageEncoder;

pub fn encode_webp_lossless(img: &Rgba) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .write_image(&img.pixels, img.width, img.height, image::ExtendedColorType::Rgba8)
        .map_err(|e| AppError::internal(format!("WebP: {e}")))?;
    Ok(out)
}

/// RGB over white (JPEG and PDF have no alpha).
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
        .write_image(&flatten_rgb(img), img.width, img.height, image::ExtendedColorType::Rgb8)
        .map_err(|e| AppError::internal(format!("JPEG: {e}")))?;
    Ok(out)
}

pub fn thumbnail_data_url(img: &Rgba) -> Option<String> {
    use base64::Engine;
    let png = encode_png(img).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

fn read_stream(stream: &InMemoryRandomAccessStream) -> AppResult<Vec<u8>> {
    let size = stream.Size().map_err(|e| platform("Поток", e))? as u32;
    let input = stream.GetInputStreamAt(0).map_err(|e| platform("Поток", e))?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| platform("Поток", e))?;
    reader.LoadAsync(size).and_then(|op| op.join()).map_err(|e| platform("Поток", e))?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).map_err(|e| platform("Поток", e))?;
    Ok(buf)
}

pub struct Pdf(PdfDocument);

impl Pdf {
    pub fn open(path: &Path) -> AppResult<Pdf> {
        let file = StorageFile::GetFileFromPathAsync(&winrt_path(path)?)
            .and_then(|op| op.join())
            .map_err(|e| platform("Не удалось открыть PDF", e))?;
        let doc = PdfDocument::LoadFromFileAsync(&file)
            .and_then(|op| op.join())
            .map_err(|e| platform("PDF повреждён или защищён паролем", e))?;
        Ok(Pdf(doc))
    }

    pub fn pages(&self) -> u32 {
        self.0.PageCount().unwrap_or(0)
    }

    /// One page as PNG bytes, `width` pixels wide.
    pub fn render_png(&self, index: u32, width: u32) -> AppResult<Vec<u8>> {
        let page = self.0.GetPage(index).map_err(|e| platform("Страница PDF", e))?;
        let stream = InMemoryRandomAccessStream::new().map_err(|e| platform("Поток", e))?;
        let opts = PdfPageRenderOptions::new().map_err(|e| platform("PDF", e))?;
        opts.SetDestinationWidth(width).map_err(|e| platform("PDF", e))?;
        page.RenderWithOptionsToStreamAsync(&stream, &opts)
            .and_then(|op| op.join())
            .map_err(|e| platform("Не удалось отрисовать страницу PDF", e))?;
        read_stream(&stream)
    }

    /// Page size in points (1/72 inch).
    pub fn page_width_pt(&self, index: u32) -> f32 {
        self.0.GetPage(index).and_then(|p| p.Size()).map(|s| s.Width).unwrap_or(612.0)
    }
}

fn ocr_engine() -> AppResult<OcrEngine> {
    // Russian first: it also reads Latin letters; the user profile languages
    // as a fallback.
    for tag in ["ru", "ru-RU"] {
        if let Ok(lang) = Language::CreateLanguage(&HSTRING::from(tag)) {
            if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
                if let Ok(e) = OcrEngine::TryCreateFromLanguage(&lang) {
                    return Ok(e);
                }
            }
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|_| AppError::unsupported("Распознавание текста недоступно: в Windows не установлен языковой пакет OCR"))
}

fn ocr_bitmap(engine: &OcrEngine, img: &Rgba) -> AppResult<String> {
    // OCR wants BGRA8.
    let mut bgra = img.pixels.clone();
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let writer = DataWriter::new().map_err(|e| platform("OCR", e))?;
    writer.WriteBytes(&bgra).map_err(|e| platform("OCR", e))?;
    let buffer = writer.DetachBuffer().map_err(|e| platform("OCR", e))?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, img.width as i32, img.height as i32)
        .map_err(|e| platform("OCR", e))?;
    let result = engine.RecognizeAsync(&bitmap).and_then(|op| op.join()).map_err(|e| platform("Не удалось распознать текст", e))?;
    let lines = result.Lines().map_err(|e| platform("OCR", e))?;
    let mut out = Vec::new();
    for line in lines {
        if let Ok(t) = line.Text() {
            out.push(t.to_string());
        }
    }
    Ok(out.join("\n"))
}

fn ocr_limit() -> u32 {
    OcrEngine::MaxImageDimension().unwrap_or(4096).max(1024)
}

/// Text on a picture.
pub fn ocr_image(path: &Path) -> AppResult<String> {
    let engine = ocr_engine()?;
    let img = decode(path, Some(ocr_limit()))?;
    ocr_bitmap(&engine, &img)
}

/// Text of every PDF page (rendered at ~200 dpi, then recognised), with a
/// progress callback per page. Works for scans and text PDFs alike.
pub fn ocr_pdf(path: &Path, mut progress: impl FnMut(u32, u32) -> bool) -> AppResult<String> {
    let engine = ocr_engine()?;
    let pdf = Pdf::open(path)?;
    let n = pdf.pages();
    let mut pages = Vec::new();
    for i in 0..n {
        if !progress(i, n) {
            return Err(AppError::Cancelled);
        }
        let width = ((pdf.page_width_pt(i) / 72.0 * 200.0) as u32).clamp(800, ocr_limit());
        let png = pdf.render_png(i, width)?;
        let img = decode_png_bytes(&png)?;
        let text = ocr_bitmap(&engine, &img)?;
        pages.push(if n > 1 { format!("— Страница {} —\n{text}", i + 1) } else { text });
    }
    Ok(pages.join("\n\n"))
}

pub fn decode_png_bytes(png: &[u8]) -> AppResult<Rgba> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|e| AppError::internal(format!("PNG: {e}")))?;
    let rgba = img.to_rgba8();
    Ok(Rgba { width: rgba.width(), height: rgba.height(), pixels: rgba.into_raw() })
}
