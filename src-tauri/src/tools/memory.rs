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
    let id = ctx.state().store.add_memory(&text)?;
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
            description: "Remember a durable fact or preference the user explicitly asked you to remember \
                (e.g. where their main project lives). Saved memories are shown to you at the start of every turn.",
            schema: object(json!({ "text": { "type": "string" } }), &["text"]),
            risk: low,
            title: |_| "Запомнить".into(),
            timeout: Duration::from_secs(3),
            parallel_safe: false,
            handler: handler(memory_save),
        },
        Tool {
            name: "memory_forget",
            description: "Delete a saved memory by id when the user asks to forget it.",
            schema: object(json!({ "id": { "type": "integer" } }), &["id"]),
            risk: low,
            title: |_| "Забыть".into(),
            timeout: Duration::from_secs(3),
            parallel_safe: false,
            handler: handler(memory_forget),
        },
    ]
}
