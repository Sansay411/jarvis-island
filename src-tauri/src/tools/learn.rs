//! Getting better with use: `learn` keeps lessons about working with this
//! user (see [`crate::learn`]), `persona_set` changes JARVIS's own manner
//! and humour when the user asks («юмор на восемьдесят процентов»).

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use super::{handler, low, object, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::learn::{self, Kind};
use crate::store::settings::STYLES;

async fn learn_tool(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let store = ctx.state().store.clone();
    match str_arg(&args, "action") {
        "forget" => {
            let id = args.get("id").and_then(Value::as_i64).ok_or_else(|| AppError::invalid("Нужен id урока"))?;
            if !learn::forget(&store, id)? {
                return Ok(ToolOutput::fail("Такого урока нет", json!({ "id": id })));
            }
            let _ = ctx.app.emit(learn::EVT, ());
            Ok(ToolOutput::ok("Забыл этот урок", json!({ "id": id })))
        }
        "save" | "" => {
            let kind = Kind::parse(str_arg(&args, "kind")).ok_or_else(|| AppError::invalid("kind: preference, hearing, alias или humor"))?;
            let (heard, meant, text) = (str_arg(&args, "heard").trim(), str_arg(&args, "meant").trim(), str_arg(&args, "text").trim());
            let (key, value, note) = match kind {
                Kind::Hearing => (heard, meant, ""),
                Kind::Alias => (heard, meant, text),
                _ => ("", text, ""),
            };
            // An alias for a device or room of the smart home works in the
            // tool calls themselves, not only in the prompt.
            let scope = if kind == Kind::Alias {
                ctx.app.try_state::<Arc<crate::mcp::Hub>>().and_then(|h| h.home_alias_scope(value)).unwrap_or_default()
            } else {
                String::new()
            };
            let saved = learn::save(&store, kind, key, value, &scope, note, "model")?;
            let _ = ctx.app.emit(learn::EVT, ());
            let summary = match kind {
                Kind::Hearing => format!("Учту: «{key}» — это «{value}»"),
                Kind::Alias => format!("Запомнил: «{key}» — это «{value}»"),
                _ => format!("Учту: {value}"),
            };
            Ok(ToolOutput::ok(summary, json!({ "id": saved.id, "existing": saved.existing })))
        }
        other => Err(AppError::invalid(format!("Неизвестное действие «{other}»: save или forget"))),
    }
}

fn style_word(style: &str) -> &'static str {
    match style {
        "sarcastic" => "сарказм",
        "playful" => "балагур",
        "warm" => "тёплая",
        "concise" => "деловая",
        _ => "с юмором",
    }
}

async fn persona_set(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let store = ctx.state().store.clone();
    let mut s = store.settings();
    let mut changed = Vec::new();
    if let Some(style) = args.get("style").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) {
        if !STYLES.contains(&style) {
            return Err(AppError::invalid(format!("Манера: {}", STYLES.join(", "))));
        }
        s.persona.style = style.to_string();
        changed.push(format!("манера «{}»", style_word(style)));
    }
    if let Some(h) = args.get("humor").and_then(Value::as_f64) {
        let h = h.round().clamp(0.0, 100.0) as u8;
        s.persona.humor = h;
        changed.push(format!("юмор {h}%"));
    }
    if let Some(a) = args.get("address").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) {
        if a != "ty" && a != "vy" {
            return Err(AppError::invalid("address: ty или vy"));
        }
        s.persona.address = a.to_string();
        changed.push(if a == "vy" { "на «вы»".to_string() } else { "на «ты»".to_string() });
    }
    if let Some(n) = args.get("user_name").and_then(Value::as_str) {
        let n = n.trim();
        if n.chars().count() > 40 {
            return Err(AppError::invalid("Слишком длинное имя"));
        }
        s.persona.user_name = n.to_string();
        changed.push(if n.is_empty() { "без имени".to_string() } else { format!("имя «{n}»") });
    }
    if changed.is_empty() {
        let p = &s.persona;
        return Ok(ToolOutput::ok(
            format!("Сейчас: манера «{}», юмор {}%", style_word(&p.style), p.humor),
            json!({ "style": p.style, "humor": p.humor, "address": p.address, "userName": p.user_name }),
        ));
    }
    store.save_settings(&s)?;
    let _ = ctx.app.emit("settings:changed", ());
    let p = &s.persona;
    Ok(ToolOutput::ok(
        format!("Характер: {}", changed.join(", ")),
        json!({ "style": p.style, "humor": p.humor, "address": p.address, "userName": p.user_name }),
    ))
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "learn",
            description: "Get better at working with this user over time. Save a lesson the moment you learn it (no need to ask; one call, then carry on): \
                kind=preference — the user corrected how you do or say something («короче», «не переспрашивай», «свет вечером делай тёплым») or praised something specific: \
                write it as a rule in Russian in `text`; kind=humor — a joke that landed (they laughed, played along) or one they disliked, in `text`; \
                kind=hearing — speech recognition misheard a word and you are sure what was meant (the user repeated or corrected it): `heard` as recognised, `meant` as said \
                (from then on the text is fixed before you see it); kind=alias — the user's own word for an app, site, room or smart-home device: `heard` = their word, \
                `meant` = the exact name that works, `text` = what it is. Facts about the user's life go to memory_save instead. \
                action=forget with `id` when a lesson turns out wrong. Your lessons are listed in every prompt.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["save", "forget"] },
                    "kind": { "type": "string", "enum": ["preference", "hearing", "alias", "humor"] },
                    "text": { "type": "string", "description": "preference/humor: the lesson as a short rule; alias: what the target is" },
                    "heard": { "type": "string", "description": "hearing: the words as recognised; alias: the user's own word" },
                    "meant": { "type": "string", "description": "hearing: what the user really said; alias: the exact name that works" },
                    "id": { "type": "integer", "description": "forget: the lesson id from the prompt" }
                }),
                &["action"],
            ),
            risk: low,
            title: |a| match a.get("action").and_then(Value::as_str) {
                Some("forget") => "Забыть урок".into(),
                _ => "Учусь".into(),
            },
            timeout: Duration::from_secs(3),
            parallel_safe: false,
            handler: handler(learn_tool),
        },
        Tool {
            name: "persona_set",
            description: "Change your own manner when the user asks for it: humour level 0–100 («юмор на восемьдесят процентов», «шути больше» → +20, \
                «будь посерьёзнее» → lower it), style (witty — dry irony; sarcastic — sharp friendly teasing; playful — jokes a lot, energetic; warm; concise — \
                business-like), address («обращайся на вы» → vy) or the user's name («зови меня Саша»). Without arguments returns the current settings. \
                Saved in Settings → «Характер», applies from your next reply — answer this time already in the new manner.",
            schema: object(
                json!({
                    "style": { "type": "string", "enum": STYLES },
                    "humor": { "type": "integer", "minimum": 0, "maximum": 100 },
                    "address": { "type": "string", "enum": ["ty", "vy"] },
                    "user_name": { "type": "string" }
                }),
                &[],
            ),
            risk: low,
            title: |_| "Характер".into(),
            timeout: Duration::from_secs(3),
            parallel_safe: false,
            handler: handler(persona_set),
        },
    ]
}
