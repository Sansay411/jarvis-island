//! Timers and reminders for the agent («поставь таймер на 25 минут»,
//! «напомни в 18:30 позвонить в банк»).

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
use tauri::Manager;

use super::{handler, low, object, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::reminders::{local_hm, parse_local, spoken_duration, Reminders};

async fn reminder_create(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let kind = if str_arg(&args, "kind") == "timer" { "timer" } else { "reminder" };
    let text = str_arg(&args, "text").trim().to_string();
    let due = if let Some(secs) = args.get("in_seconds").and_then(Value::as_u64).filter(|s| *s > 0) {
        Utc::now() + chrono::Duration::seconds(secs.min(366 * 24 * 3600) as i64)
    } else if let Some(at) = args.get("at_local").and_then(Value::as_str) {
        parse_local(at).ok_or_else(|| AppError::invalid("Время нужно в виде «ГГГГ-ММ-ДД ЧЧ:ММ» или «ЧЧ:ММ»"))?
    } else {
        return Err(AppError::invalid("Нужно in_seconds или at_local"));
    };
    if kind == "reminder" && text.is_empty() {
        return Err(AppError::invalid("О чём напомнить?"));
    }
    let r = ctx.app.state::<Arc<Reminders>>().create(kind, &text, due)?;
    let left = (due - Utc::now()).num_seconds().max(1) as u64;
    let summary = match kind {
        "timer" => format!("Таймер на {} — до {}", spoken_duration(left.div_ceil(5) * 5), local_hm(&due)),
        _ => format!("Напомню в {}: {}", local_hm(&due), text),
    };
    Ok(ToolOutput::ok(summary, json!({ "id": r.id, "dueAt": r.due_at })))
}

async fn reminders_list(ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    let list = ctx.app.state::<Arc<Reminders>>().list();
    let items: Vec<Value> = list
        .iter()
        .map(|r| {
            let due = chrono::DateTime::parse_from_rfc3339(&r.due_at).map(|d| d.with_timezone(&chrono::Local).format("%d.%m %H:%M").to_string()).unwrap_or_default();
            json!({ "id": r.id, "kind": r.kind, "text": r.text, "due": due, "fired": r.fired_at.is_some() })
        })
        .collect();
    let summary = if items.is_empty() { "Активных таймеров и напоминаний нет".to_string() } else { format!("Активных: {}", items.len()) };
    Ok(ToolOutput::ok(summary, json!({ "items": items })))
}

async fn reminder_cancel(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let id = args.get("id").and_then(Value::as_i64).ok_or_else(|| AppError::invalid("Нужен id"))?;
    let reminders = ctx.app.state::<Arc<Reminders>>();
    if !reminders.list().iter().any(|r| r.id == id) {
        return Err(AppError::not_found("Такого активного напоминания нет"));
    }
    reminders.done(id)?;
    Ok(ToolOutput::ok("Отменил", json!({ "id": id })))
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "reminder_create",
            description: "Set a timer or a reminder that fires on this PC (island alert, spoken if voice is on); survives restarts. \
                kind=timer for countdowns («таймер на 10 минут», text optional), kind=reminder with text for «напомни …». \
                Give either in_seconds (relative) or at_local in the user's local time «YYYY-MM-DD HH:MM» or «HH:MM». \
                Ask the user when the time is ambiguous («после обеда»).",
            schema: object(
                json!({
                    "kind": { "type": "string", "enum": ["timer", "reminder"] },
                    "text": { "type": "string" },
                    "in_seconds": { "type": "integer", "minimum": 1 },
                    "at_local": { "type": "string" }
                }),
                &["kind"],
            ),
            risk: low,
            title: |a| if str_arg(a, "kind") == "timer" { "Таймер".into() } else { "Напоминание".into() },
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(reminder_create),
        },
        Tool {
            name: "reminders_list",
            description: "List active timers and reminders (id, text, local due time, whether it already fired).",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Напоминания".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: true,
            handler: handler(reminders_list),
        },
        Tool {
            name: "reminder_cancel",
            description: "Cancel (or mark done) a timer or reminder by id from reminders_list.",
            schema: object(json!({ "id": { "type": "integer" } }), &["id"]),
            risk: low,
            title: |_| "Отменить напоминание".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(reminder_cancel),
        },
    ]
}
