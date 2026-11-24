//! Timers and reminders for the agent («поставь таймер на 25 минут»,
//! «напомни в 18:30 позвонить в банк»).

use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, Utc};
use serde_json::{json, Value};
use tauri::Manager;

use super::{handler, low, object, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::reminders::{local_hm, parse_local, spoken_duration, Reminders};
use crate::repeat::{capitalize_first, describe_rule, format_next_local, next_after, RepeatRule};

async fn reminder_create(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let kind = match str_arg(&args, "kind") {
        "timer" => "timer",
        "action" => "action",
        _ => "reminder",
    };
    let text = str_arg(&args, "text").trim().to_string();
    let repeat: Option<RepeatRule> = match args.get("repeat").filter(|v| !v.is_null()) {
        Some(v) => {
            let mut rule: RepeatRule = serde_json::from_value(v.clone())
                .map_err(|e| AppError::invalid(format!("Некорректное правило повторения: {e}")))?;
            rule.validate_and_normalize().map_err(AppError::invalid)?;
            Some(rule)
        }
        None => None,
    };

    let now_local = Local::now();
    let due = if let Some(secs) = args.get("in_seconds").and_then(Value::as_u64).filter(|s| *s > 0) {
        Utc::now() + chrono::Duration::seconds(secs.min(366 * 24 * 3600) as i64)
    } else if let Some(at) = args.get("at_local").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
        parse_local(at).ok_or_else(|| AppError::invalid("Время нужно в виде «ГГГГ-ММ-ДД ЧЧ:ММ» или «ЧЧ:ММ»"))?
    } else if let Some(rule) = repeat.as_ref() {
        next_after(rule, now_local, 0)
            .ok_or_else(|| AppError::invalid("У этого правила повторения нет будущих срабатываний"))?
            .with_timezone(&Utc)
    } else {
        return Err(AppError::invalid("Нужно in_seconds, at_local или repeat"));
    };

    if kind != "timer" && text.is_empty() {
        return Err(AppError::invalid(if kind == "action" { "Что сделать?" } else { "О чём напомнить?" }));
    }

    let r = ctx.app.state::<Arc<Reminders>>().create_with_repeat(kind, &text, due, repeat.as_ref())?;
    let summary = if let Some(rule) = repeat.as_ref() {
        let rule_desc = describe_rule(rule);
        let next_str = format_next_local(due.with_timezone(&Local), now_local);
        match kind {
            "action" => format!("{} сделаю: {}. Ближайшее — {}", capitalize_first(&rule_desc), text, next_str),
            "timer" if text.is_empty() => format!("Повторяющийся таймер ({}). Ближайшее — {}", rule_desc, next_str),
            "timer" => format!("Повторяющийся таймер ({}): {}. Ближайшее — {}", rule_desc, text, next_str),
            _ => format!("Буду напоминать {}: {}. Ближайшее — {}", rule_desc, text, next_str),
        }
    } else {
        let left = (due - Utc::now()).num_seconds().max(1) as u64;
        match kind {
            "timer" => format!("Таймер на {} — до {}", spoken_duration(left.div_ceil(5) * 5), local_hm(&due)),
            "action" => format!("В {} сделаю: {}", local_hm(&due), text),
            _ => format!("Напомню в {}: {}", local_hm(&due), text),
        }
    };
    let note = (kind == "action").then_some("Runs by itself at dueAt. Do not do it now or in any later turn before then.");
    Ok(ToolOutput::ok(
        summary,
        json!({ "id": r.id, "dueAt": r.due_at, "repeat": r.repeat_label, "note": note }),
    ))
}

async fn reminders_list(ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    let list = ctx.app.state::<Arc<Reminders>>().list();
    let items: Vec<Value> = list
        .iter()
        .map(|r| {
            let due = chrono::DateTime::parse_from_rfc3339(&r.due_at)
                .map(|d| d.with_timezone(&Local).format("%d.%m %H:%M").to_string())
                .unwrap_or_default();
            json!({
                "id": r.id,
                "kind": r.kind,
                "text": r.text,
                "due": due,
                "fired": r.fired_at.is_some(),
                "repeat": r.repeat_label,
                "repeating": r.repeat.is_some()
            })
        })
        .collect();
    let reminders = ctx.app.state::<Arc<Reminders>>();
    let summary = match reminders.next_timer() {
        Some(t) => {
            let left = chrono::DateTime::parse_from_rfc3339(&t.due_at).map(|d| (d.with_timezone(&Utc) - Utc::now()).num_seconds().max(0) as u64).unwrap_or(0);
            let more = if items.len() > 1 { format!(" Всего активных: {}.", items.len()) } else { String::new() };
            format!("До конца таймера {}.{more}", spoken_duration(left.max(1)))
        }
        None if items.is_empty() => "Активных таймеров и напоминаний нет".to_string(),
        None => format!("Активных напоминаний: {}", items.len()),
    };
    Ok(ToolOutput::ok(summary, json!({ "items": items })))
}

async fn reminder_cancel(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let reminders = ctx.app.state::<Arc<Reminders>>();
    // No id: «отмени таймер» means the running timer that ends first.
    let id = match args.get("id").and_then(Value::as_i64) {
        Some(id) => id,
        None => reminders.next_timer().map(|t| t.id).ok_or_else(|| AppError::not_found("Запущенных таймеров нет"))?,
    };
    let existing = reminders
        .list()
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| AppError::not_found("Такого активного напоминания нет"))?;
    reminders.cancel(id)?;
    let msg = if existing.repeat.is_some() { "Отменил серию" } else { "Отменил" };
    Ok(ToolOutput::ok(msg, json!({ "id": id })))
}

async fn reminder_skip(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let id = args
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::invalid("Укажи id повторяющегося напоминания"))?;
    let next = ctx.app.state::<Arc<Reminders>>().skip(id)?;
    let summary = match next {
        Some(n) => format!("Пропустил ближайшее. Следующее — {}", format_next_local(n.with_timezone(&Local), Local::now())),
        None => "Это было последнее срабатывание, серия завершена".to_string(),
    };
    Ok(ToolOutput::ok(summary, json!({ "id": id, "nextDueAt": next.map(|n| n.to_rfc3339()) })))
}

async fn stopwatch(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let sw = ctx.app.state::<Arc<crate::stopwatch::Stopwatch>>();
    let s = match str_arg(&args, "action") {
        "start" => sw.start(),
        "pause" => sw.pause(),
        "lap" => sw.lap(),
        "reset" => sw.reset(),
        _ => sw.status(),
    };
    let time = crate::stopwatch::spoken(s.elapsed_ms());
    let summary = match str_arg(&args, "action") {
        "start" if s.accumulated_ms == 0 => "Секундомер запущен".to_string(),
        "start" => format!("Секундомер продолжает: {time}"),
        "pause" if s.active => format!("Секундомер остановлен: {time}"),
        "lap" if s.active => format!("Круг {}: {time}", s.laps.len()),
        "reset" => "Секундомер сброшен".to_string(),
        _ if !s.active => "Секундомер не запущен".to_string(),
        _ => format!("На секундомере {time}{}", if s.running { "" } else { ", он на паузе" }),
    };
    Ok(ToolOutput::ok(summary, json!({ "running": s.running, "elapsed_ms": s.elapsed_ms(), "laps": s.laps })))
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "reminder_create",
            description: "Set a one-shot or repeating timer, reminder or scheduled action that fires on this PC (island alert, spoken if voice is on); survives restarts. \
                kind=timer for countdowns («таймер на 10 минут», text optional), kind=reminder with text for «напомни …». \
                kind=action when the user wants something DONE later or on a schedule («через полчаса закрой ТикТок», «каждый день в 8 утра собери новости тех и пришли сводку»): \
                text is the instruction in the user's words, and at each occurrence you will carry it out yourself with your tools. \
                Never do the action now when it was asked for later or on a schedule. \
                Give in_seconds (relative), at_local («YYYY-MM-DD HH:MM» or «HH:MM»), and/or repeat. If only repeat is given, the first occurrence is computed automatically. \
                Examples for repeat: \
                «каждый день в 8» → {kind:'daily', at:'08:00'}; \
                «по будням в 9» → {kind:'weekdays', at:'09:00'}; \
                «по выходным в 11» → {kind:'weekly', days:['sat','sun'], at:'11:00'}; \
                «каждый понедельник в 10» → {kind:'weekly', days:['mon'], at:'10:00'}; \
                «каждые полчаса с 9 до 18» → {kind:'interval', every_minutes:30, from:'09:00', to:'18:00'}; \
                «первого числа каждого месяца в 12» → {kind:'monthly', day:1, at:'12:00'}. \
                Ask the user when the time is ambiguous («после обеда»).",
            schema: object(
                json!({
                    "kind": { "type": "string", "enum": ["timer", "reminder", "action"] },
                    "text": { "type": "string" },
                    "in_seconds": { "type": "integer", "minimum": 1 },
                    "at_local": { "type": "string", "description": "Optional first occurrence in local time: 'YYYY-MM-DD HH:MM' or 'HH:MM'." },
                    "repeat": {
                        "type": "object",
                        "description": "Optional recurrence rule in local wall time.",
                        "properties": {
                            "kind": { "type": "string", "enum": ["daily", "weekdays", "weekly", "monthly", "interval"] },
                            "at": { "type": "string", "description": "Local wall time 'HH:MM' (for daily, weekdays, weekly, monthly)." },
                            "days": {
                                "type": "array",
                                "items": { "type": "string", "enum": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] },
                                "description": "Weekdays (required for weekly; optional filter for interval)."
                            },
                            "day": { "type": "integer", "minimum": 1, "maximum": 31, "description": "Day of month 1..31 for monthly (shorter months fire on their last day)." },
                            "every_minutes": { "type": "integer", "minimum": 5, "description": "Interval in minutes (minimum 5, required for interval)." },
                            "from": { "type": "string", "description": "Optional daily window start 'HH:MM' for interval." },
                            "to": { "type": "string", "description": "Optional daily window end 'HH:MM' for interval (may be earlier than from to cross midnight)." },
                            "until": { "type": "string", "description": "Optional inclusive end date 'YYYY-MM-DD'." },
                            "count": { "type": "integer", "minimum": 1, "description": "Optional total number of occurrences." }
                        },
                        "required": ["kind"],
                        "additionalProperties": false
                    }
                }),
                &["kind"],
            ),
            risk: low,
            title: |a| {
                let repeating = a.get("repeat").is_some_and(|v| !v.is_null());
                match str_arg(a, "kind") {
                    "timer" => "Таймер".into(),
                    "action" if repeating => "Задача по расписанию".into(),
                    "action" => "Отложенное действие".into(),
                    _ if repeating => "Повтор напоминания".into(),
                    _ => "Напоминание".into(),
                }
            },
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(reminder_create),
        },
        Tool {
            name: "reminders_list",
            description: "List active timers, reminders and scheduled actions (id, kind, text, local due time, repeat rule, whether it already fired).",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Напоминания".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: true,
            handler: handler(reminders_list),
        },
        Tool {
            name: "reminder_cancel",
            description: "Cancel a timer, reminder or the entire repeating series by id from reminders_list; without id — the running timer that ends first.",
            schema: object(json!({ "id": { "type": "integer" } }), &[]),
            risk: low,
            title: |_| "Отменить напоминание".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(reminder_cancel),
        },
        Tool {
            name: "reminder_skip",
            description: "Skip only the next occurrence of a repeating reminder or scheduled action by id, keeping the rest of the series.",
            schema: object(json!({ "id": { "type": "integer" } }), &["id"]),
            risk: low,
            title: |_| "Пропустить напоминание".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(reminder_skip),
        },
        Tool {
            name: "stopwatch",
            description: "The stopwatch on the island: start (or resume), pause, lap, reset, status (elapsed time).",
            schema: object(json!({ "action": { "type": "string", "enum": ["start", "pause", "lap", "reset", "status"] } }), &["action"]),
            risk: low,
            title: |a| match str_arg(a, "action") {
                "start" => "Запустить секундомер".into(),
                "pause" => "Остановить секундомер".into(),
                "lap" => "Отметить круг".into(),
                "reset" => "Сбросить секундомер".into(),
                _ => "Секундомер".into(),
            },
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(stopwatch),
        },
    ]
}
