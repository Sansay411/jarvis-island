//! The island's file actions for the agent: «отправь этот файл в избранное»,
//! «переведи видео в mp4», «расшифруй голосовое» by voice or chat.

use std::time::Duration;

use tauri::Manager;

use serde_json::{json, Value};

use super::{handler, object, Risk, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};

const ACTIONS: &[&str] = &[
    "telegram", "zip", "unzip", "convert:png", "convert:jpg", "convert:webp", "strip_meta", "to_pdf", "pdf:png", "ocr", "extract_text",
    "remux:mp4", "remux:mkv", "extract_audio", "frame", "compress", "convert:flac", "convert:wav", "convert:mp3", "transcribe", "copy_files",
    "reveal", "open",
];

fn paths_of(args: &Value) -> Vec<String> {
    args.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

async fn file_action(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let action = args.get("action").and_then(Value::as_str).unwrap_or("").to_string();
    if !ACTIONS.contains(&action.as_str()) {
        return Err(AppError::invalid(format!("Неизвестное действие «{action}». Доступны: {}", ACTIONS.join(", "))));
    }
    let mut paths = paths_of(&args);
    if paths.is_empty() {
        paths = ctx.attachments.iter().cloned().collect();
    }
    if paths.is_empty() {
        paths = crate::filekit::SHOWN.lock().clone();
    }
    if paths.is_empty() {
        return Err(AppError::invalid("Не указаны файлы — перетащите их на остров или назовите путь"));
    }
    let app = ctx.app.clone();
    let cancel = ctx.cancel.clone();
    let act = action.clone();
    let out = crate::platform::blocking(move || {
        let mut progress = |_x: f32| !cancel.is_cancelled();
        let voice = app.state::<std::sync::Arc<crate::voice::VoiceManager>>();
        crate::filekit::execute(Some(voice.inner().as_ref()), &paths, &act, &mut progress)
    })
    .await?;
    let text = out.text.map(|t| t.chars().take(6000).collect::<String>());
    Ok(ToolOutput::ok(out.message, json!({ "outputs": out.outputs, "text": text })))
}

fn risk(args: &Value) -> Risk {
    match args.get("action").and_then(Value::as_str) {
        // Leaves the computer: always confirmed.
        Some("telegram") => Risk::High,
        Some("open") => Risk::Moderate,
        _ => Risk::Low,
    }
}

pub fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "file_action",
        description: "Run a file action on local files (the user's attachments when `paths` is empty). Actions: \
            telegram — send the original files to the user's own Telegram «Избранное» (Saved Messages) via Telegram Desktop; \
            zip / unzip; convert:png|jpg|webp (images; png and webp are lossless); strip_meta (remove EXIF/GPS from JPG/PNG without re-encoding); \
            to_pdf (images → one PDF); pdf:png (PDF pages → PNG); ocr (text from images/PDF, copied to clipboard); extract_text (docx/xlsx/pptx); \
            remux:mp4|mkv (change video container without re-encoding); extract_audio (audio track without re-encoding); frame (PNG still); \
            compress (re-encode video smaller, lossy); convert:flac|wav (lossless) | convert:mp3 (lossy); transcribe (speech → .txt, plus .srt for video, recognised locally); \
            copy_files (files to clipboard); reveal; open. Results are new files next to the originals; originals are never changed.",
        schema: object(
            json!({
                "action": { "type": "string", "enum": ACTIONS },
                "paths": { "type": "array", "items": { "type": "string" }, "description": "Absolute paths; empty = the attached files" }
            }),
            &["action"],
        ),
        risk,
        title: |a| crate::filekit::title_for(a.get("action").and_then(Value::as_str).unwrap_or(""), paths_of(a).len().max(1)),
        timeout: Duration::from_secs(1800),
        parallel_safe: false,
        handler: handler(file_action),
    }]
}
