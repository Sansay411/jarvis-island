//! ZIP archives, text from Office documents (they are ZIPs of XML), and
//! metadata removal that keeps the image data byte for byte.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

use crate::error::{AppError, AppResult};

fn io(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Storage(format!("{what}: {e}"))
}

/// Pack files and folders (recursively) into `out`. `progress(done, total)`
/// returns false to cancel.
pub fn zip_paths(paths: &[PathBuf], out: &Path, mut progress: impl FnMut(usize, usize) -> bool) -> AppResult<usize> {
    let mut entries: Vec<(PathBuf, String)> = Vec::new();
    for p in paths {
        let base = p.parent().unwrap_or(Path::new(""));
        if p.is_dir() {
            let mut stack = vec![p.clone()];
            while let Some(dir) = stack.pop() {
                let rel = dir.strip_prefix(base).unwrap_or(&dir).to_string_lossy().replace('\\', "/");
                entries.push((dir.clone(), format!("{rel}/")));
                for e in std::fs::read_dir(&dir).map_err(|e| io("Папка", e))?.flatten() {
                    let path = e.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                        entries.push((path, rel));
                    }
                }
            }
        } else {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
            entries.push((p.clone(), name));
        }
    }
    let file = File::create(out).map_err(|e| io("Не удалось создать архив", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let total = entries.len();
    let mut files = 0;
    for (i, (path, name)) in entries.iter().enumerate() {
        if !progress(i, total) {
            drop(zip);
            let _ = std::fs::remove_file(out);
            return Err(AppError::Cancelled);
        }
        if name.ends_with('/') {
            zip.add_directory(name.trim_end_matches('/'), opts).map_err(|e| io("Архив", e))?;
            continue;
        }
        zip.start_file(name, opts).map_err(|e| io("Архив", e))?;
        let mut src = File::open(path).map_err(|e| io(&format!("Не удалось прочитать {}", path.display()), e))?;
        std::io::copy(&mut src, &mut zip).map_err(|e| io("Архив", e))?;
        files += 1;
    }
    zip.finish().map_err(|e| io("Архив", e))?;
    Ok(files)
}

/// Limits that keep a small "zip bomb" from filling the disk.
const UNZIP_MAX_FILES: usize = 100_000;
const UNZIP_MAX_ENTRY: u64 = 8 << 30;
const UNZIP_MAX_TOTAL: u64 = 16 << 30;

/// Extract into `dir` (created; removed again on failure or cancel). Entries
/// that would escape the folder, links and oversized content are refused.
/// `progress(fraction)` returns false to cancel.
pub fn unzip(archive: &Path, dir: &Path, mut progress: impl FnMut(f32) -> bool) -> AppResult<usize> {
    let file = File::open(archive).map_err(|e| io("Архив", e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| AppError::invalid("Это не ZIP-архив или он повреждён"))?;
    if zip.len() > UNZIP_MAX_FILES {
        return Err(AppError::unsupported(format!("В архиве больше {UNZIP_MAX_FILES} файлов, распаковывать не стану")));
    }
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| io("Архив", e))?;
        if entry.enclosed_name().is_none() {
            return Err(AppError::invalid(format!("В архиве небезопасный путь «{}» — распаковка отменена", entry.name())));
        }
        if entry.encrypted() {
            return Err(AppError::unsupported("Архив защищён паролем"));
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| io("Папка", e))?;
    let res = extract_limited(&mut zip, dir, &mut progress);
    if res.is_err() {
        let _ = std::fs::remove_dir_all(dir);
    }
    res
}

fn extract_limited(zip: &mut zip::ZipArchive<File>, dir: &Path, progress: &mut impl FnMut(f32) -> bool) -> AppResult<usize> {
    let total = zip.len();
    let mut written: u64 = 0;
    for i in 0..total {
        if !progress(i as f32 / total.max(1) as f32) {
            return Err(AppError::Cancelled);
        }
        let mut entry = zip.by_index(i).map_err(|e| io("Архив", e))?;
        let Some(rel) = entry.enclosed_name() else { continue };
        // Symbolic links could point the next entries outside the folder.
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            continue;
        }
        let out = dir.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| io("Папка", e))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io("Папка", e))?;
        }
        // The declared size can lie: count what is really written.
        let budget = UNZIP_MAX_ENTRY.min(UNZIP_MAX_TOTAL - written);
        let mut dst = File::create(&out).map_err(|e| io("Не удалось распаковать", e))?;
        let n = std::io::copy(&mut (&mut entry).take(budget + 1), &mut dst).map_err(|e| io("Не удалось распаковать", e))?;
        if n > budget {
            return Err(AppError::unsupported("Распакованные данные слишком велики (больше 8 ГБ на файл или 16 ГБ всего), распаковка остановлена"));
        }
        written += n;
    }
    Ok(total)
}

/// Plain text of a .docx / .xlsx / .pptx (paragraphs, cells, slides).
pub fn office_text(path: &Path) -> AppResult<String> {
    let file = File::open(path).map_err(|e| io("Файл", e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| AppError::invalid("Документ повреждён"))?;
    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    let mut parts: Vec<String> = names
        .iter()
        .filter(|n| {
            *n == "word/document.xml"
                || n.starts_with("word/footnotes")
                || n == &"xl/sharedStrings.xml"
                || (n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        })
        .cloned()
        .collect();
    // Slides in their order: slide2 before slide10.
    parts.sort_by_key(|n| (n.len(), n.clone()));
    if parts.is_empty() {
        return Err(AppError::unsupported("В этом документе нет текста, который я умею читать"));
    }
    let mut out = Vec::new();
    for name in parts {
        let mut xml = String::new();
        // 64 MB of XML is far beyond any real document; more is a zip bomb.
        zip.by_name(&name).map_err(|e| io("Документ", e))?.take(64 << 20).read_to_string(&mut xml).map_err(|e| io("Документ", e))?;
        out.push(xml_text(&xml));
    }
    Ok(out.join("\n\n").trim().to_string())
}

/// Text nodes of WordprocessingML/SpreadsheetML/DrawingML; paragraph and
/// cell ends become line breaks.
fn xml_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        out.push_str(&unescape(&rest[..lt]));
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        if matches!(tag, "/w:p" | "/a:p" | "/si" | "w:br/" | "a:br/" | "w:br") {
            out.push('\n');
        } else if tag == "w:tab/" {
            out.push('\t');
        }
        rest = &rest[lt + gt + 1..];
    }
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// JPEG without EXIF/XMP/IPTC/comments: APP1, APP13 and COM segments are
/// dropped, every other byte (the compressed image itself) is kept.
pub fn strip_jpeg(data: &[u8]) -> AppResult<Vec<u8>> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(AppError::invalid("Это не JPEG"));
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            return Err(AppError::invalid("JPEG повреждён"));
        }
        let marker = data[i + 1];
        if marker == 0xDA {
            // Start of scan: the rest is image data.
            out.extend_from_slice(&data[i..]);
            return Ok(out);
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if i + 2 + len > data.len() {
            return Err(AppError::invalid("JPEG повреждён"));
        }
        let keep = !matches!(marker, 0xE1 | 0xED | 0xFE);
        if keep {
            out.extend_from_slice(&data[i..i + 2 + len]);
        }
        i += 2 + len;
    }
    Err(AppError::invalid("JPEG без данных изображения"))
}

/// PNG without text and EXIF chunks (tEXt, zTXt, iTXt, eXIf, tIME).
pub fn strip_png(data: &[u8]) -> AppResult<Vec<u8>> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !data.starts_with(SIG) {
        return Err(AppError::invalid("Это не PNG"));
    }
    let mut out = SIG.to_vec();
    let mut i = SIG.len();
    while i + 12 <= data.len() {
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let end = i + 12 + len;
        if end > data.len() {
            return Err(AppError::invalid("PNG повреждён"));
        }
        let kind = &data[i + 4..i + 8];
        if !matches!(kind, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME") {
            out.extend_from_slice(&data[i..end]);
        }
        i = end;
        if kind == b"IEND" {
            return Ok(out);
        }
    }
    Err(AppError::invalid("PNG повреждён"))
}

/// Size and colour components of a baseline/progressive JPEG (SOF marker).
pub fn jpeg_info(data: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i = 2;
    while i + 9 < data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let marker = data[i + 1];
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return Some((w, h, data[i + 9]));
        }
        i += 2 + len;
    }
    None
}

/// Pages for `images_to_pdf`.
pub enum PdfImage {
    /// JPEG bytes embedded as they are (DCTDecode) — no recompression.
    Jpeg { data: Vec<u8>, width: u32, height: u32, gray: bool },
    /// 8-bit RGB, compressed losslessly (Flate).
    Rgb { data: Vec<u8>, width: u32, height: u32 },
}

/// A PDF with one image per page, the page sized to the picture (72 dpi →
/// 1 px = 1 pt, capped to A4-ish 842 pt on the long side).
pub fn images_to_pdf(images: &[PdfImage], out: &Path) -> AppResult<()> {
    use flate2::write::ZlibEncoder;
    let mut buf: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    buf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let n = images.len();
    // Objects: 1 catalog, 2 pages, then per page: page, image, content.
    let obj = |buf: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(buf.len());
        let id = offsets.len();
        buf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut buf, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + i * 3)).collect();
    obj(&mut buf, &mut offsets, format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).as_bytes());
    for (i, img) in images.iter().enumerate() {
        let (w, h) = match img {
            PdfImage::Jpeg { width, height, .. } | PdfImage::Rgb { width, height, .. } => (*width as f32, *height as f32),
        };
        let k = (842.0 / w.max(h)).min(1.0);
        let (pw, ph) = (w * k, h * k);
        let (page, image, content) = (3 + i * 3, 4 + i * 3, 5 + i * 3);
        obj(
            &mut buf,
            &mut offsets,
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /XObject << /Im{i} {image} 0 R >> >> /Contents {content} 0 R >>").as_bytes(),
        );
        let _ = page;
        let mut body = Vec::new();
        match img {
            PdfImage::Jpeg { data, width, height, gray } => {
                let cs = if *gray { "/DeviceGray" } else { "/DeviceRGB" };
                body.extend_from_slice(
                    format!("<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", data.len()).as_bytes(),
                );
                body.extend_from_slice(data);
            }
            PdfImage::Rgb { data, width, height } => {
                let mut z = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                z.write_all(data).map_err(|e| io("PDF", e))?;
                let packed = z.finish().map_err(|e| io("PDF", e))?;
                body.extend_from_slice(
                    format!("<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", packed.len()).as_bytes(),
                );
                body.extend_from_slice(&packed);
            }
        }
        body.extend_from_slice(b"\nendstream");
        obj(&mut buf, &mut offsets, &body);
        let draw = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im{i} Do Q");
        obj(&mut buf, &mut offsets, format!("<< /Length {} >>\nstream\n{draw}\nendstream", draw.len()).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    std::fs::write(out, buf).map_err(|e| io("Не удалось записать PDF", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_xml_to_text() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Привет,</w:t></w:r><w:r><w:t xml:space="preserve"> мир &amp; всё</w:t></w:r></w:p><w:p><w:r><w:t>Вторая</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(xml_text(xml).trim(), "Привет, мир & всё\nВторая");
    }

    #[test]
    fn jpeg_metadata_is_removed_and_image_data_kept() {
        // SOI, APP0(JFIF, len 4), APP1(EXIF, len 6), SOS + data, EOI.
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 1, 2, 0xFF, 0xE1, 0, 6, b'E', b'x', b'i', b'f', 0xFF, 0xDA, 0, 2, 9, 9, 0xFF, 0xD9];
        let out = strip_jpeg(&jpeg).unwrap();
        assert_eq!(out, vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 1, 2, 0xFF, 0xDA, 0, 2, 9, 9, 0xFF, 0xD9]);
    }

    #[test]
    fn zip_roundtrip_in_temp() {
        let dir = std::env::temp_dir().join(format!("jarvis-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/inner")).unwrap();
        std::fs::write(dir.join("src/a.txt"), "альфа").unwrap();
        std::fs::write(dir.join("src/inner/b.txt"), "бета").unwrap();
        let out = dir.join("out.zip");
        assert_eq!(zip_paths(&[dir.join("src")], &out, |_, _| true).unwrap(), 2);
        let n = unzip(&out, &dir.join("x"), |_| true).unwrap();
        assert!(n >= 2);
        assert_eq!(std::fs::read_to_string(dir.join("x/src/inner/b.txt")).unwrap(), "бета");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pdf_from_rgb_is_well_formed() {
        let dir = std::env::temp_dir();
        let out = dir.join(format!("jarvis-pdf-{}.pdf", std::process::id()));
        images_to_pdf(&[PdfImage::Rgb { data: vec![200; 4 * 3 * 3], width: 4, height: 3 }], &out).unwrap();
        let b = std::fs::read(&out).unwrap();
        assert!(b.starts_with(b"%PDF-1.4"));
        assert!(b.ends_with(b"%%EOF\n"));
        let _ = std::fs::remove_file(out);
    }
}
