//! `weather` and `location`: real weather for where the user is (or a named
//! place), shown as a card on the island; the user's city for «рядом» questions.

use std::time::Duration;

use serde_json::{json, Value};
use tauri::Emitter;

use super::{handler, low, object, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::platform;

/// The island shows the card for a few seconds.
pub const EVT_WEATHER: &str = "weather:show";

async fn weather(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let place = str_arg(&args, "place").trim().to_string();
    let tomorrow = str_arg(&args, "day") == "tomorrow";
    let w = platform::blocking(move || crate::geo::weather((!place.is_empty()).then_some(place.as_str()))).await?;
    let mut data = serde_json::to_value(&w).map_err(|e| AppError::internal(e.to_string()))?;
    // The island opens on the day the answer is about.
    let mut card = data.clone();
    card["focus"] = json!(if tomorrow && w.tomorrow.is_some() { "tomorrow" } else { "today" });
    let _ = ctx.app.emit(EVT_WEATHER, &card);
    if let Some(o) = data.as_object_mut() {
        // The model reads the summary; hour lists for the other day are noise.
        if tomorrow { o.remove("hours"); } else { o.remove("tomorrowHours"); }
    }
    Ok(ToolOutput::ok(crate::geo::summary(&w), data))
}

async fn location(_: ToolCtx, _: Value) -> AppResult<ToolOutput> {
    let p = platform::blocking(crate::geo::here).await?;
    Ok(ToolOutput::ok(
        format!("Пользователь сейчас примерно в: {}, {}, {} (по интернет-адресу ПК, точность — город)", p.city, p.region, p.country),
        json!(p),
    ))
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "weather",
            description: "Current weather and the forecast for today, the next hours and tomorrow. Without `place` it uses where the user is now (their city by the PC's network) — use that for «какая погода», «нужен ли зонт», «что надеть». Pass `place` only when the user names another city. Set `day` to \"tomorrow\" when the question is about tomorrow («погода на завтра», «что завтра надеть») — the island card then opens on tomorrow. Prefer this over web_search for weather.",
            schema: object(
                json!({
                    "place": { "type": "string", "maxLength": 80, "description": "City named by the user; omit for the user's own location" },
                    "day": { "type": "string", "enum": ["today", "tomorrow"], "description": "Which day the user asks about; default today" }
                }),
                &[],
            ),
            risk: low,
            title: |a| {
                let p = a.get("place").and_then(Value::as_str).unwrap_or("").trim();
                let day = if a.get("day").and_then(Value::as_str) == Some("tomorrow") { " на завтра" } else { "" };
                if p.is_empty() { format!("Погода{day}") } else { format!("Погода{day}: {p}") }
            },
            timeout: Duration::from_secs(20),
            parallel_safe: true,
            handler: handler(weather),
        },
        Tool {
            name: "location",
            description: "The user's current city, region and country (by the PC's network, city-level). Use it for anything about «здесь», «рядом», «у нас»: local news, places nearby, opening hours, local time of events — then put the city into web_search queries.",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Где я".into(),
            timeout: Duration::from_secs(15),
            parallel_safe: true,
            handler: handler(location),
        },
    ]
}
