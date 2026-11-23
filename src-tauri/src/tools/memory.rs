//! Explicit user memories ("запомни, что …"). Only what the user asked to
//! remember is stored; the list is visible and deletable in settings.

use std::time::Duration;

use serde_json::{json, Value};

use super::{handler, low, object, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};

async fn memory_save(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let text = str_arg(&args, "text").trim().to_string();
    if text.is_empty() {
        return Err(AppError::invalid("Пустая заметка"));
    }
    let store = &ctx.state().store;
    // The same fact twice is noise in every later prompt.
    let norm = |t: &str| t.to_lowercase().replace('ё', "е").split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    if let Some(known) = store.memories()?.into_iter().find(|m| norm(&m.text) == norm(&text)) {
        return Ok(ToolOutput::ok(format!("Уже помню: {}", known.text), json!({ "id": known.id, "existing": true })));
    }
    let id = store.add_memory(&text)?;
    Ok(ToolOutput::ok(format!("Запомнил: {text}"), json!({ "id": id })))
}

async fn memory_forget(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let id = args.get("id").and_then(Value::as_i64).ok_or_else(|| AppError::invalid("Нужен id"))?;
    ctx.state().store.delete_memory(id)?;
    Ok(ToolOutput::ok("Забыл", json!({ "id": id })))
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "memory_save",
            description: "Save a lasting fact about the user for all future conversations: who they are, their city, work, \
                people close to them, projects, habits, tastes. Save it when the user shares it (no need to be asked), as one \
                short sentence in Russian (e.g. «Работает дизайнером, живёт в Караганде»). Skip what your memory already has, \
                passing moods, one-off requests and anything secret (passwords, keys, card numbers). Saved memories are shown \
                to you at the start of every turn.",
            schema: object(json!({ "text": { "type": "string" } }), &["text"]),
            risk: low,
            title: |_| "Запомнить".into(),
            timeout: Duration::from_secs(3),
            parallel_safe: false,
            handler: handler(memory_save),
        },
        Tool {
            name: "memory_forget",
            description: "Delete a saved memory by id: when the user asks to forget it, or when the fact changed (then save the new one).",
            schema: object(json!({ "id": { "type": "integer" } }), &["id"]),
            risk: low,
            title: |_| "Забыть".into(),
            timeout: Duration::from_secs(3),
            parallel_safe: false,
            handler: handler(memory_forget),
        },
    ]
}
