//! Files dropped on the island: what they are and what can be done with
//! them. The type comes from the file's signature, not just its extension;
//! each type gets a short list of actions. Nothing happens on drop — only
//! when the user picks an action. Originals are never changed: results are
//! new files next to them («имя (1).png» when the name is taken), checked
//! after writing (exists, non-empty, readable) before «готово» is reported.

pub mod archive;
pub mod media;
pub mod telegram;
pub mod wic;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};

pub const EVT_JOB: &str = "files:job";

/// Files on the island card right now: «этот файл» in a spoken or typed
/// request means them (the agent sees the list in its context).
pub static SHOWN: std::sync::LazyLock<Mutex<Vec<String>>> = std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileAction {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub ext: String,
    /// image | video | audio | pdf | document | spreadsheet | presentation |
    /// archive | text | code | folder | other
    pub kind: &'static str,
    pub size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_s: Option<f64>,
    pub pages: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub files: Vec<FileInfo>,
    pub actions: Vec<FileAction>,
    /// FFmpeg found: audio/video actions possible.
    pub ffmpeg: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub job_id: String,
    pub action: String,
    pub title: String,
    /// running | done | failed | cancelled
    pub state: &'static str,
    pub progress: Option<f32>,
    pub message: Option<String>,
    pub outputs: Vec<String>,
    /// Recognised or extracted text (also put on the clipboard).
    pub text: Option<String>,
}

const RAW_EXT: &[&str] = &["cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf", "srw", "pef"];
const TEXT_EXT: &[&str] = &["txt", "md", "csv", "tsv", "json", "log", "xml", "yaml", "yml", "ini", "toml", "rtf", "srt", "vtt"];
const CODE_EXT: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "py", "c", "cc", "cpp", "h", "hpp", "cs", "go", "java", "kt", "swift", "php", "rb", "sh",
    "ps1", "bat", "cmd", "sql", "html", "htm", "css", "scss", "vue", "svelte", "lua", "dart",
];

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// What the file is, by its first bytes (extension only to disambiguate
/// containers: ZIP → docx/xlsx, MP4 → m4a, Matroska → mka).
pub fn detect(path: &Path) -> &'static str {
    if path.is_dir() {
        return "folder";
    }
    let ext = ext_of(path);
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let h = &head[..n];
    let at = |i: usize, sig: &[u8]| h.len() >= i + sig.len() && &h[i..i + sig.len()] == sig;
    if at(0, b"\x89PNG") || at(0, b"\xFF\xD8\xFF") || at(0, b"GIF8") || at(0, b"BM") || at(0, b"II*\0") || at(0, b"MM\0*") || (at(0, b"RIFF") && at(8, b"WEBP")) || at(0, &[0, 0, 1, 0]) {
        // TIFF signatures are shared by some RAW formats — still images.
        return "image";
    }
    if at(4, b"ftyp") {
        let brand = h.get(8..12).unwrap_or(&[]);
        return match brand {
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" | b"avif" | b"avis" => "image",
            b"M4A " | b"M4B " | b"M4P " => "audio",
            _ if matches!(ext.as_str(), "m4a" | "m4b" | "aac") => "audio",
            _ => "video",
        };
    }
    if at(0, b"%PDF") {
        return "pdf";
    }
    if at(0, b"PK\x03\x04") {
        return match ext.as_str() {
            "docx" | "docm" | "odt" => "document",
            "xlsx" | "xlsm" | "ods" => "spreadsheet",
            "pptx" | "ppsx" | "odp" => "presentation",
            _ => "archive",
        };
    }
    if at(0, b"7z\xBC\xAF\x27\x1C") || at(0, b"Rar!") || at(0, &[0x1F, 0x8B]) {
        return "archive";
    }
    if at(0, b"OggS") {
        return if ext == "ogv" { "video" } else { "audio" };
    }
    if at(0, b"fLaC") || at(0, b"ID3") || at(0, b"#!AMR") || (at(0, b"RIFF") && at(8, b"WAVE")) || (h.len() >= 2 && h[0] == 0xFF && h[1] & 0xE0 == 0xE0 && ext != "jpg") {
        return "audio";
    }
    if (at(0, b"RIFF") && at(8, b"AVI ")) || at(0, &[0x1A, 0x45, 0xDF, 0xA3]) || at(0, &[0x30, 0x26, 0xB2, 0x75]) {
        return match ext.as_str() {
            "mka" | "weba" | "wma" => "audio",
            _ => "video",
        };
    }
    if RAW_EXT.contains(&ext.as_str()) {
        return "image";
    }
    if matches!(ext.as_str(), "doc") {
        return "document";
    }
    if CODE_EXT.contains(&ext.as_str()) {
        return "code";
    }
    if TEXT_EXT.contains(&ext.as_str()) || (n > 0 && std::str::from_utf8(h).is_ok() && !h.contains(&0)) {
        return "text";
    }
    "other"
}

fn act(id: &str, label: &str, hint: Option<&str>, primary: bool) -> FileAction {
    FileAction { id: id.into(), label: label.into(), hint: hint.map(str::to_string), primary }
}

/// The actions shown for one file, primary first.
pub fn actions_for(info: &FileInfo, ffmpeg: bool) -> Vec<FileAction> {
    let mut a = Vec::new();
    let ext = info.ext.as_str();
    match info.kind {
        "image" => {
            a.push(act("chat", "В чат", Some("Джарвис посмотрит"), true));
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            if ext != "png" {
                let hint = if matches!(ext, "jpg" | "jpeg" | "heic" | "heif" | "avif") { "без потерь при сохранении" } else { "без потерь" };
                a.push(act("convert:png", "В PNG", Some(hint), true));
            }
            a.push(act("ocr", "Текст", Some("распознать и скопировать"), true));
            if !matches!(ext, "jpg" | "jpeg") {
                a.push(act("convert:jpg", "В JPG", Some("с потерями, для совместимости"), false));
            }
            if ext != "webp" {
                a.push(act("convert:webp", "В WebP", Some("без потерь"), false));
            }
            if matches!(ext, "jpg" | "jpeg" | "png") {
                a.push(act("strip_meta", "Убрать метаданные", Some("EXIF и GPS, картинка не меняется"), false));
            }
            a.push(act("to_pdf", "В PDF", None, false));
        }
        "video" if ffmpeg => {
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            if info.audio_codec.is_some() {
                a.push(act("extract_audio", "Звук", Some("дорожка без перекодирования"), true));
            }
            let probe = media::Probe { video: info.video_codec.clone(), audio: info.audio_codec.clone(), ..Default::default() };
            let mp4_ok = media::mp4_compatible(&probe);
            if ext != "mp4" && mp4_ok {
                a.push(act("remux:mp4", "В MP4", Some("без перекодирования"), true));
            } else if ext != "mkv" {
                a.push(act("remux:mkv", "В MKV", Some("без перекодирования"), true));
            }
            if info.audio_codec.is_some() {
                a.push(act("transcribe", "Расшифровать", Some("текст и субтитры на этом ПК"), true));
            }
            a.push(act("frame", "Кадр", Some("PNG"), false));
            a.push(act("compress", "Сжать", Some("с потерями, для отправки"), false));
            if ext == "mp4" && ext != "mkv" {
                a.push(act("remux:mkv", "В MKV", Some("без перекодирования"), false));
            }
        }
        "audio" if ffmpeg => {
            a.push(act("transcribe", "Расшифровать", Some("текст на этом ПК"), true));
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            if ext != "flac" {
                a.push(act("convert:flac", "В FLAC", Some("без потерь"), true));
            }
            if ext != "wav" {
                a.push(act("convert:wav", "В WAV", Some("без потерь"), false));
            }
            if ext != "mp3" {
                a.push(act("convert:mp3", "В MP3", Some("с потерями, для совместимости"), false));
            }
        }
        "video" | "audio" => {
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            a.push(act("need_ffmpeg", "Нужен FFmpeg", Some("winget install Gyan.FFmpeg"), false));
        }
        "pdf" => {
            a.push(act("chat", "В чат", Some("Джарвис прочитает"), true));
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            a.push(act("ocr", "Текст", Some("распознать и скопировать"), true));
            a.push(act("pdf:png", "Страницы в PNG", None, true));
        }
        "archive" => {
            if ext == "zip" {
                a.push(act("unzip", "Распаковать", Some("в папку рядом"), true));
            }
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
        }
        "document" | "spreadsheet" | "presentation" => {
            a.push(act("chat", "В чат", Some("Джарвис прочитает"), true));
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            if !matches!(ext, "doc") {
                a.push(act("extract_text", "Текст", Some("скопировать"), true));
            }
        }
        "text" | "code" => {
            a.push(act("chat", "В чат", Some("Джарвис прочитает"), true));
            a.push(act("ask:summarize", "Кратко", Some("о чём файл"), true));
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            a.push(act("ask:translate", "Перевести", None, false));
        }
        "folder" => {
            a.push(act("zip", "В ZIP", Some("архив рядом"), true));
            a.push(act("reveal", "Показать", Some("в Проводнике"), true));
            a.push(act("copy_path", "Путь", Some("скопировать"), true));
        }
        _ => {
            a.push(act("telegram", "В Избранное", Some("Telegram, оригинал"), true));
            a.push(act("chat", "В чат", None, true));
        }
    }
    if info.kind != "folder" {
        a.push(act("open", "Открыть", None, false));
        a.push(act("reveal", "Показать в папке", None, false));
        a.push(act("copy_files", "Копировать файл", Some("вставить куда угодно"), false));
        a.push(act("copy_path", "Копировать путь", None, false));
        if info.kind != "archive" {
            a.push(act("zip", "В ZIP", None, false));
        }
    }
    a
}

/// Actions for several files at once.
pub fn actions_for_many(files: &[FileInfo], ffmpeg: bool) -> Vec<FileAction> {
    let all = |k: &str| files.iter().all(|f| f.kind == k);
    let mut a = Vec::new();
    if !files.iter().any(|f| f.kind == "folder") {
        a.push(act("telegram", "В Избранное", Some("все оригиналы"), true));
        a.push(act("chat", "В чат", None, true));
    }
    a.push(act("zip", "В ZIP", Some("один архив"), true));
    if all("image") {
        a.push(act("to_pdf", "В один PDF", Some("JPEG вставляется как есть"), true));
        a.push(act("convert:png", "Все в PNG", Some("без потерь"), false));
        a.push(act("convert:jpg", "Все в JPG", Some("с потерями"), false));
    }
    if ffmpeg && all("audio") {
        a.push(act("transcribe", "Расшифровать все", None, true));
        a.push(act("convert:flac", "Все в FLAC", Some("без потерь"), false));
    }
    if ffmpeg && all("video") {
        a.push(act("extract_audio", "Звук из всех", Some("без перекодирования"), false));
    }
    a.push(act("copy_files", "Копировать файлы", Some("вставить куда угодно"), false));
    a.push(act("copy_path", "Копировать пути", None, false));
    a
}

fn info_of(path: &Path, ffmpeg: bool) -> FileInfo {
    let meta = std::fs::metadata(path).ok();
    let kind = detect(path);
    let mut info = FileInfo {
        path: path.to_string_lossy().to_string(),
        name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        ext: ext_of(path),
        kind,
        size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()).unwrap_or(0),
        width: None,
        height: None,
        duration_s: None,
        pages: None,
        video_codec: None,
        audio_codec: None,
        thumbnail: None,
    };
    match kind {
        "image" => {
            if let Ok(img) = wic::decode(path, Some(160)) {
                info.thumbnail = wic::thumbnail_data_url(&img);
            }
            if let Ok((w, h)) = wic::dimensions(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        "video" | "audio" if ffmpeg => {
            if let Ok(p) = media::probe(path, Duration::from_secs(3)) {
                info.duration_s = p.duration_s;
                info.video_codec = p.video.clone();
                info.audio_codec = p.audio.clone();
                info.width = p.width;
                info.height = p.height;
                if kind == "video" && p.video.is_some() {
                    let at = p.duration_s.map(|d| (d * 0.1).min(3.0)).unwrap_or(0.0);
                    if let Ok(png) = media::frame_png(path, at, Some(160), Duration::from_secs(3)) {
                        use base64::Engine;
                        info.thumbnail = Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)));
                    }
                }
            }
        }
        "pdf" => {
            if let Ok(pdf) = wic::Pdf::open(path) {
                info.pages = Some(pdf.pages());
                if let Ok(png) = pdf.render_png(0, 160) {
                    use base64::Engine;
                    info.thumbnail = Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)));
                }
            }
        }
        _ => {}
    }
    info
}

/// Blocking: call through `platform::blocking` (COM + WinRT on the thread).
pub fn inspect(paths: &[String]) -> AppResult<Inspection> {
    if paths.is_empty() {
        return Err(AppError::invalid("Нет файлов"));
    }
    let ffmpeg = media::available();
    let mut files = Vec::new();
    for p in paths {
        let path = Path::new(p);
        if !path.exists() {
            return Err(AppError::not_found(format!("Файл не найден: {p}")));
        }
        files.push(info_of(path, ffmpeg));
    }
    let actions = if files.len() == 1 { actions_for(&files[0], ffmpeg) } else { actions_for_many(&files, ffmpeg) };
    Ok(Inspection { files, actions, ffmpeg })
}

/// `dir/stem.ext`, or `stem (1).ext`, `stem (2).ext`… — never an existing file.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let name = |n: u32| if n == 0 { format!("{stem}.{ext}") } else { format!("{stem} ({n}).{ext}") };
    let name_dir = |n: u32| if n == 0 { stem.to_string() } else { format!("{stem} ({n})") };
    for n in 0..1000 {
        let p = if ext.is_empty() { dir.join(name_dir(n)) } else { dir.join(name(n)) };
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", uuid::Uuid::new_v4().simple()))
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "файл".into())
}

fn parent_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// A written result must exist and not be empty.
fn verified(p: PathBuf) -> AppResult<PathBuf> {
    match std::fs::metadata(&p) {
        Ok(m) if m.is_dir() || m.len() > 0 => Ok(p),
        _ => {
            let _ = std::fs::remove_file(&p);
            Err(AppError::platform(format!("Результат не записался: {}", p.display())))
        }
    }
}

pub struct Outcome {
    pub message: String,
    pub outputs: Vec<String>,
    pub text: Option<String>,
}

/// Progress reporter: fraction 0..1; returns false once cancelled.
pub type Progress<'a> = &'a mut dyn FnMut(f32) -> bool;

pub fn title_for(action: &str, n: usize) -> String {
    let what = match action {
        "telegram" => "Отправляю в Избранное",
        "zip" => "Сжимаю в ZIP",
        "unzip" => "Распаковываю",
        "ocr" => "Распознаю текст",
        "transcribe" => "Расшифровываю",
        "to_pdf" => "Собираю PDF",
        "pdf:png" => "Сохраняю страницы",
        "extract_audio" => "Извлекаю звук",
        "frame" => "Сохраняю кадр",
        "compress" => "Сжимаю видео",
        "strip_meta" => "Убираю метаданные",
        "extract_text" => "Достаю текст",
        a if a.starts_with("convert:") || a.starts_with("remux:") => "Конвертирую",
        _ => "Выполняю",
    };
    if n > 1 { format!("{what} · {}", plural(n, "файл", "файла", "файлов")) } else { what.to_string() }
}

/// Run one action on the files (blocking, COM initialised by the caller).
pub fn execute(voice: Option<&crate::voice::VoiceManager>, paths: &[String], action: &str, progress: Progress<'_>) -> AppResult<Outcome> {
    let list: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let first = list.first().ok_or_else(|| AppError::invalid("Нет файлов"))?.clone();
    for p in &list {
        if !p.exists() {
            return Err(AppError::not_found(format!("Файл не найден: {}", p.display())));
        }
    }
    let n = list.len();
    let each = |progress: Progress<'_>, f: &mut dyn FnMut(&Path, Progress<'_>) -> AppResult<PathBuf>| -> AppResult<Vec<String>> {
        let mut outs = Vec::new();
        for (i, p) in list.iter().enumerate() {
            let base = i as f32 / n as f32;
            let mut sub = |x: f32| progress(base + x / n as f32);
            outs.push(verified(f(p, &mut sub)?)?.to_string_lossy().to_string());
        }
        Ok(outs)
    };
    let done = |message: String, outputs: Vec<String>| Ok(Outcome { message, outputs, text: None });
    match action {
        "open" => {
            for p in &list {
                crate::platform::apps::shell_open(&p.to_string_lossy(), None, None)?;
            }
            done(if n > 1 { format!("Открыл {}", plural(n, "файл", "файла", "файлов")) } else { "Открыл".into() }, vec![])
        }
        "reveal" => {
            let arg = format!("/select,\"{}\"", std::path::absolute(&first).unwrap_or(first.clone()).display());
            { use std::os::windows::process::CommandExt; std::process::Command::new("explorer.exe").raw_arg(&arg).spawn() }.map_err(|e| AppError::platform(format!("Проводник: {e}")))?;
            done("Показал в Проводнике".into(), vec![])
        }
        "copy_path" => {
            crate::platform::clipboard::write_text(&paths.join("\r\n"))?;
            done(if n > 1 { "Пути скопированы".into() } else { "Путь скопирован".into() }, vec![])
        }
        "copy_files" => {
            crate::platform::clipboard::write_files(paths)?;
            done(if n > 1 { format!("{} в буфере — вставьте Ctrl+V", plural(n, "файл", "файла", "файлов")) } else { "Файл в буфере — вставьте Ctrl+V".into() }, vec![])
        }
        "telegram" => {
            let sent = telegram::send_to_saved(paths)?;
            done(format!("Передал в Telegram, чат «{}»: {}. Загрузку видно в самом Telegram.", sent.chat, plural(sent.count, "файл", "файла", "файлов")), vec![])
        }
        "zip" => {
            let stem = if n == 1 { stem_of(&first) } else { parent_of(&first).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Архив".into()) };
            let out = unique_path(&parent_of(&first), &stem, "zip");
            let files = archive::zip_paths(&list, &out, |i, t| progress(i as f32 / t.max(1) as f32))?;
            let out = verified(out)?;
            done(format!("Архив готов: {} ({})", out.file_name().unwrap_or_default().to_string_lossy(), plural(files, "файл", "файла", "файлов")), vec![out.to_string_lossy().to_string()])
        }
        "unzip" => {
            let dir = unique_path(&parent_of(&first), &stem_of(&first), "");
            let count = archive::unzip(&first, &dir, &mut *progress)?;
            done(format!("Распаковал {} в «{}»", plural(count, "элемент", "элемента", "элементов"), dir.file_name().unwrap_or_default().to_string_lossy()), vec![dir.to_string_lossy().to_string()])
        }
        "convert:png" | "convert:jpg" | "convert:webp" => {
            let ext = &action["convert:".len()..];
            let outs = each(progress, &mut |p, _| {
                let img = wic::decode(p, None)?;
                let bytes = match ext {
                    "png" => wic::encode_png(&img)?,
                    "jpg" => wic::encode_jpeg(&img, 95)?,
                    _ => wic::encode_webp_lossless(&img)?,
                };
                let out = unique_path(&parent_of(p), &stem_of(p), ext);
                std::fs::write(&out, bytes).map_err(|e| AppError::Storage(format!("Запись: {e}")))?;
                // Read the result back: it must decode to the same size.
                let back = wic::dimensions(&out)?;
                if back != (img.width, img.height) {
                    return Err(AppError::platform("Проверка результата не прошла: размер изображения не совпал"));
                }
                Ok(out)
            })?;
            done(format!("Готово: {}", names(&outs)), outs)
        }
        "strip_meta" => {
            let outs = each(progress, &mut |p, _| {
                let data = std::fs::read(p).map_err(|e| AppError::Storage(e.to_string()))?;
                let (clean, ext) = match ext_of(p).as_str() {
                    "jpg" | "jpeg" => (archive::strip_jpeg(&data)?, "jpg"),
                    "png" => (archive::strip_png(&data)?, "png"),
                    _ => return Err(AppError::unsupported("Метаданные убираю только у JPG и PNG")),
                };
                let out = unique_path(&parent_of(p), &format!("{} (без метаданных)", stem_of(p)), ext);
                std::fs::write(&out, clean).map_err(|e| AppError::Storage(e.to_string()))?;
                wic::dimensions(&out)?;
                Ok(out)
            })?;
            done(format!("Без метаданных: {}", names(&outs)), outs)
        }
        "to_pdf" => {
            let mut pages = Vec::new();
            for (i, p) in list.iter().enumerate() {
                if !progress(i as f32 / n as f32 * 0.9) {
                    return Err(AppError::Cancelled);
                }
                let data = std::fs::read(p).map_err(|e| AppError::Storage(e.to_string()))?;
                match archive::jpeg_info(&data).filter(|(_, _, c)| *c == 1 || *c == 3) {
                    Some((w, h, c)) if data.starts_with(&[0xFF, 0xD8]) && wic::dimensions(p).is_ok_and(|d| d == (w, h)) => {
                        pages.push(archive::PdfImage::Jpeg { data, width: w, height: h, gray: c == 1 })
                    }
                    _ => {
                        let img = wic::decode(p, None)?;
                        pages.push(archive::PdfImage::Rgb { data: wic::flatten_rgb(&img), width: img.width, height: img.height });
                    }
                }
            }
            let out = unique_path(&parent_of(&first), &stem_of(&first), "pdf");
            archive::images_to_pdf(&pages, &out)?;
            let out = verified(out)?;
            let check = wic::Pdf::open(&out)?.pages();
            if check as usize != pages.len() {
                return Err(AppError::platform("Проверка PDF не прошла: не все страницы читаются"));
            }
            done(format!("PDF готов: {} ({check} стр.)", out.file_name().unwrap_or_default().to_string_lossy()), vec![out.to_string_lossy().to_string()])
        }
        "pdf:png" => {
            let pdf = wic::Pdf::open(&first)?;
            let count = pdf.pages();
            let dir = unique_path(&parent_of(&first), &format!("{} — страницы", stem_of(&first)), "");
            std::fs::create_dir_all(&dir).map_err(|e| AppError::Storage(e.to_string()))?;
            for i in 0..count {
                if !progress(i as f32 / count.max(1) as f32) {
                    return Err(AppError::Cancelled);
                }
                let width = ((pdf.page_width_pt(i) / 72.0 * 200.0) as u32).clamp(600, 4000);
                let png = pdf.render_png(i, width)?;
                std::fs::write(dir.join(format!("стр. {:03}.png", i + 1)), png).map_err(|e| AppError::Storage(e.to_string()))?;
            }
            done(format!("Сохранил {count} стр. в «{}»", dir.file_name().unwrap_or_default().to_string_lossy()), vec![dir.to_string_lossy().to_string()])
        }
        "ocr" | "extract_text" => {
            let mut texts = Vec::new();
            for (i, p) in list.iter().enumerate() {
                let kind = detect(p);
                let base = i as f32 / n as f32;
                let text = match (action, kind) {
                    (_, "pdf") => wic::ocr_pdf(p, |pg, total| progress(base + pg as f32 / total.max(1) as f32 / n as f32))?,
                    (_, "image") => wic::ocr_image(p)?,
                    (_, "document" | "spreadsheet" | "presentation") => archive::office_text(p)?,
                    (_, "text" | "code") => std::fs::read_to_string(p).map_err(|e| AppError::Storage(e.to_string()))?,
                    _ => return Err(AppError::unsupported("Текст достаю из картинок, PDF и документов Office")),
                };
                texts.push(if n > 1 { format!("— {} —\n{text}", p.file_name().unwrap_or_default().to_string_lossy()) } else { text });
            }
            let text = texts.join("\n\n").trim().to_string();
            if text.is_empty() {
                return Ok(Outcome { message: "Текста не нашёл".into(), outputs: vec![], text: None });
            }
            crate::platform::clipboard::write_text(&text)?;
            let words = text.split_whitespace().count();
            Ok(Outcome { message: format!("Текст скопирован в буфер ({words} слов)"), outputs: vec![], text: Some(text) })
        }
        "remux:mp4" | "remux:mkv" | "extract_audio" | "frame" | "compress" | "convert:flac" | "convert:wav" | "convert:mp3" => {
            let outs = each(progress, &mut |p, prog| ffmpeg_action(p, action, prog))?;
            done(format!("Готово: {}", names(&outs)), outs)
        }
        "transcribe" => {
            let voice = voice.ok_or_else(|| AppError::unsupported("Расшифровка доступна только в приложении"))?;
            let mut all_text = Vec::new();
            let mut outs = Vec::new();
            for (i, p) in list.iter().enumerate() {
                let base = i as f32 / n as f32;
                // Progress by decoded time against the file's length; a
                // cancel stops FFmpeg and recognition together.
                let duration = media::probe(p, std::time::Duration::from_secs(10)).ok().and_then(|pr| pr.duration_s).filter(|d| *d > 0.0);
                let mut decoded = 0usize;
                let phrases = voice.transcribe_stream(|feed| {
                    media::decode_pcm16k(p, |samples| {
                        feed(samples)?;
                        decoded += samples.len();
                        let x = duration.map_or(0.0, |d| (decoded as f64 / 16_000.0 / d).min(1.0) as f32);
                        Ok(progress(base + x / n as f32))
                    })
                })?;
                if phrases.is_empty() {
                    continue;
                }
                let text = phrases.iter().map(|(_, _, t)| t.as_str()).collect::<Vec<_>>().join(" ");
                let txt = unique_path(&parent_of(p), &stem_of(p), "txt");
                std::fs::write(&txt, &text).map_err(|e| AppError::Storage(e.to_string()))?;
                outs.push(verified(txt)?.to_string_lossy().to_string());
                if detect(p) == "video" {
                    let srt: String = phrases
                        .iter()
                        .enumerate()
                        .map(|(k, (s, e, t))| format!("{}\n{} --> {}\n{}\n", k + 1, media::srt_time(*s), media::srt_time(*e), t))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let out = unique_path(&parent_of(p), &stem_of(p), "srt");
                    std::fs::write(&out, srt).map_err(|e| AppError::Storage(e.to_string()))?;
                    outs.push(verified(out)?.to_string_lossy().to_string());
                }
                all_text.push(if n > 1 { format!("— {} —\n{text}", p.file_name().unwrap_or_default().to_string_lossy()) } else { text });
            }
            if all_text.is_empty() {
                return Ok(Outcome { message: "Речи в записи не нашёл".into(), outputs: vec![], text: None });
            }
            let text = all_text.join("\n\n");
            let _ = crate::platform::clipboard::write_text(&text);
            let words = text.split_whitespace().count();
            Ok(Outcome { message: format!("Расшифровка готова: {words} слов, текст в буфере"), outputs: outs, text: Some(text) })
        }
        other => Err(AppError::invalid(format!("Неизвестное действие: {other}"))),
    }
}

/// «1 файл», «3 файла», «5 файлов».
pub fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let (d, h) = (n % 10, n % 100);
    let w = if d == 1 && h != 11 { one } else if (2..=4).contains(&d) && !(12..=14).contains(&h) { few } else { many };
    format!("{n} {w}")
}

fn names(outs: &[String]) -> String {
    let n: Vec<String> = outs.iter().map(|o| Path::new(o).file_name().unwrap_or_default().to_string_lossy().to_string()).collect();
    if n.len() > 3 { format!("{} и ещё {}", n[..3].join(", "), n.len() - 3) } else { n.join(", ") }
}

fn ffmpeg_action(p: &Path, action: &str, progress: Progress<'_>) -> AppResult<PathBuf> {
    let probe = media::probe(p, Duration::from_secs(5))?;
    let dir = parent_of(p);
    let stem = stem_of(p);
    let input = p.to_string_lossy().to_string();
    let (out, args): (PathBuf, Vec<String>) = match action {
        "remux:mp4" => {
            if !media::mp4_compatible(&probe) {
                return Err(AppError::unsupported("Эти кодеки MP4 не держит без перекодирования — выберите MKV"));
            }
            let out = unique_path(&dir, &stem, "mp4");
            (out.clone(), vec!["-i".into(), input, "-map".into(), "0:v?".into(), "-map".into(), "0:a?".into(), "-c".into(), "copy".into(), "-movflags".into(), "+faststart".into(), "-n".into(), out.to_string_lossy().into()])
        }
        "remux:mkv" => {
            let out = unique_path(&dir, &stem, "mkv");
            (out.clone(), vec!["-i".into(), input, "-map".into(), "0".into(), "-c".into(), "copy".into(), "-n".into(), out.to_string_lossy().into()])
        }
        "extract_audio" => {
            let codec = probe.audio.clone().ok_or_else(|| AppError::invalid("В видео нет звука"))?;
            let out = unique_path(&dir, &stem, media::audio_container(&codec));
            (out.clone(), vec!["-i".into(), input, "-map".into(), "0:a:0".into(), "-vn".into(), "-c:a".into(), "copy".into(), "-n".into(), out.to_string_lossy().into()])
        }
        "frame" => {
            let at = probe.duration_s.map(|d| (d / 2.0).min(1.0)).unwrap_or(0.0);
            let out = unique_path(&dir, &format!("{stem} — кадр"), "png");
            (out.clone(), vec!["-ss".into(), format!("{at:.2}"), "-i".into(), input, "-frames:v".into(), "1".into(), "-n".into(), out.to_string_lossy().into()])
        }
        "compress" => {
            let out = unique_path(&dir, &format!("{stem} (сжато)"), "mp4");
            (
                out.clone(),
                vec![
                    "-i".into(), input, "-map".into(), "0:v:0".into(), "-map".into(), "0:a:0?".into(), "-c:v".into(), "libx264".into(), "-crf".into(), "23".into(),
                    "-preset".into(), "veryfast".into(), "-pix_fmt".into(), "yuv420p".into(), "-c:a".into(), "aac".into(), "-b:a".into(), "160k".into(),
                    "-movflags".into(), "+faststart".into(), "-n".into(), out.to_string_lossy().into(),
                ],
            )
        }
        "convert:flac" | "convert:wav" | "convert:mp3" => {
            if probe.audio.is_none() {
                return Err(AppError::invalid("В файле нет звука"));
            }
            let ext = &action["convert:".len()..];
            let out = unique_path(&dir, &stem, ext);
            let mut args: Vec<String> = vec!["-i".into(), input, "-map".into(), "0:a:0".into(), "-vn".into()];
            match ext {
                "flac" => args.extend(["-c:a".into(), "flac".into(), "-compression_level".into(), "8".into()]),
                "wav" => args.extend(["-c:a".into(), if probe.audio_bits.is_some_and(|b| b > 16) { "pcm_s24le".into() } else { "pcm_s16le".into() }]),
                _ => args.extend(["-c:a".into(), "libmp3lame".into(), "-q:a".into(), "0".into()]),
            }
            args.extend(["-n".into(), out.to_string_lossy().into()]);
            (out, args)
        }
        _ => return Err(AppError::invalid("Неизвестное действие")),
    };
    let duration = if action == "frame" { None } else { probe.duration_s };
    media::run(&args, duration, |x| progress(x))?;
    let out = verified(out)?;
    // The result must be readable media (and for copies: the same length).
    if action != "frame" {
        let back = media::probe(&out, Duration::from_secs(5)).map_err(|_| AppError::platform("Результат не читается — проверка не прошла"))?;
        if let (Some(a), Some(b)) = (probe.duration_s, back.duration_s) {
            if action != "compress" && (a - b).abs() > 1.5 {
                return Err(AppError::platform(format!("Проверка не прошла: длительность {b:.1} с вместо {a:.1} с")));
            }
        }
    }
    Ok(out)
}

/// Running file jobs (cancel by id). Events: `files:job`.
#[derive(Default)]
pub struct FileJobs {
    jobs: Mutex<HashMap<String, CancellationToken>>,
}

impl FileJobs {
    pub fn cancel(&self, id: &str) -> bool {
        self.jobs.lock().get(id).map(|c| c.cancel()).is_some()
    }

    /// Start `action` in the background; returns the job id at once.
    pub fn start(self: &Arc<Self>, app: AppHandle, paths: Vec<String>, action: String) -> String {
        let id = format!("file-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let cancel = CancellationToken::new();
        self.jobs.lock().insert(id.clone(), cancel.clone());
        let title = title_for(&action, paths.len());
        let me = Arc::clone(self);
        let job_id = id.clone();
        tauri::async_runtime::spawn(async move {
            let ev = |state, progress, message: Option<String>, outputs: Vec<String>, text: Option<String>| JobEvent {
                job_id: job_id.clone(),
                action: action.clone(),
                title: title.clone(),
                state,
                progress,
                message,
                outputs,
                text,
            };
            let _ = app.emit(EVT_JOB, ev("running", Some(0.0), None, vec![], None));
            let (app2, act2, c2, id2) = (app.clone(), action.clone(), cancel.clone(), job_id.clone());
            let res = crate::platform::blocking(move || {
                let mut last = std::time::Instant::now();
                let mut progress = |x: f32| {
                    if c2.is_cancelled() {
                        return false;
                    }
                    if last.elapsed() >= Duration::from_millis(120) {
                        last = std::time::Instant::now();
                        let _ = app2.emit(EVT_JOB, JobEvent { job_id: id2.clone(), action: act2.clone(), title: title_for(&act2, paths.len()), state: "running", progress: Some(x.clamp(0.0, 1.0)), message: None, outputs: vec![], text: None });
                    }
                    true
                };
                let voice = app2.state::<Arc<crate::voice::VoiceManager>>();
                execute(Some(voice.inner().as_ref()), &paths, &act2, &mut progress)
            })
            .await;
            me.jobs.lock().remove(&job_id);
            let event = match res {
                Ok(o) => ev("done", Some(1.0), Some(o.message), o.outputs, o.text),
                Err(AppError::Cancelled) => ev("cancelled", None, Some("Отменено".into()), vec![], None),
                Err(e) => ev("failed", None, Some(e.to_string()), vec![], None),
            };
            tracing::info!(job = %event.job_id, action = %event.action, state = event.state, "file action finished");
            let _ = app.emit(EVT_JOB, event);
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jarvis-detect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn detects_by_signature_not_extension() {
        assert_eq!(detect(&temp("photo.txt", b"\xFF\xD8\xFF\xE0rest")), "image");
        assert_eq!(detect(&temp("a.heic", b"\0\0\0\x18ftypheic\0\0")), "image");
        assert_eq!(detect(&temp("v.mov", b"\0\0\0\x14ftypqt  \0\0")), "video");
        assert_eq!(detect(&temp("s.m4a", b"\0\0\0\x20ftypM4A \0\0")), "audio");
        assert_eq!(detect(&temp("r.docx", b"PK\x03\x04....")), "document");
        assert_eq!(detect(&temp("r.zip", b"PK\x03\x04....")), "archive");
        assert_eq!(detect(&temp("d.pdf", b"%PDF-1.7")), "pdf");
        assert_eq!(detect(&temp("voice.ogg", b"OggS\0\x02")), "audio");
        assert_eq!(detect(&temp("main.rs", b"fn main() {}")), "code");
        assert_eq!(detect(&temp("notes", b"plain words")), "text");
    }

    #[test]
    fn unique_names_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("jarvis-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        std::fs::write(dir.join("a (1).png"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a", "png"), dir.join("a (2).png"));
        assert_eq!(unique_path(&dir, "b", "png"), dir.join("b.png"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn primary_actions_fit_the_type() {
        let mut f = FileInfo { path: "x.jpg".into(), name: "x.jpg".into(), ext: "jpg".into(), kind: "image", size: 1, width: None, height: None, duration_s: None, pages: None, video_codec: None, audio_codec: None, thumbnail: None };
        let ids: Vec<String> = actions_for(&f, true).into_iter().filter(|a| a.primary).map(|a| a.id).collect();
        assert_eq!(ids, ["chat", "telegram", "convert:png", "ocr"]);
        f.kind = "video";
        f.ext = "mkv".into();
        f.video_codec = Some("h264".into());
        f.audio_codec = Some("aac".into());
        let ids: Vec<String> = actions_for(&f, true).into_iter().filter(|a| a.primary).map(|a| a.id).collect();
        assert_eq!(ids, ["telegram", "extract_audio", "remux:mp4", "transcribe"]);
        let none: Vec<String> = actions_for(&f, false).into_iter().map(|a| a.id).collect();
        assert!(none.contains(&"need_ffmpeg".to_string()) && !none.contains(&"transcribe".to_string()));
    }
}
