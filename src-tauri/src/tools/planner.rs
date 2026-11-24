//! Calendar and mentor tools: «поставь тренировку на завтра в 18:00»,
//! «что у меня сегодня», «спланируй мне день», «сколько я сидел в тиктоке».

use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as CDur, Local, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

use super::{handler, low, object, parse, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::mentor::Mentor;
use crate::planner::{EventInput, EventPatch, Planner};

#[derive(Deserialize)]
struct CalArgs {
    action: String,
    id: Option<String>,
    title: Option<String>,
    start: Option<String>,
    end: Option<String>,
    all_day: Option<bool>,
    category: Option<String>,
    color: Option<String>,
    notes: Option<String>,
    repeat: Option<String>,
    from: Option<String>,
    to: Option<String>,
    date: Option<String>,
    /// `suggest`: what the user wants from the day, in their words.
    wishes: Option<String>,
    /// `plan`: the blocks the agent decided on.
    blocks: Option<Vec<crate::planner::suggest::Block>>,
    /// `free`: how many days from `date` (1..7).
    days: Option<u32>,
}

fn day(s: Option<&str>) -> AppResult<NaiveDate> {
    let today = Local::now().date_naive();
    match s.map(str::trim).unwrap_or("") {
        "" | "today" | "сегодня" => Ok(today),
        "tomorrow" | "завтра" => Ok(today + CDur::days(1)),
        other => NaiveDate::parse_from_str(other.get(..10).unwrap_or(other), "%Y-%m-%d").map_err(|_| AppError::invalid(format!("Дата «{other}» ожидается как ГГГГ-ММ-ДД"))),
    }
}

/// The matching event by id, else by title among the coming weeks' events.
fn resolve(p: &Planner, id: Option<&str>, title: Option<&str>) -> AppResult<String> {
    if let Some(id) = id.filter(|i| !i.trim().is_empty()) {
        return Ok(id.to_string());
    }
    let q = title.unwrap_or("").trim().to_lowercase();
    if q.is_empty() {
        return Err(AppError::invalid("Нужен id или название события"));
    }
    let today = Local::now().date_naive();
    let list = p.list(&(today - CDur::days(7)).format("%Y-%m-%d").to_string(), &(today + CDur::days(60)).format("%Y-%m-%d").to_string())?;
    list.iter().find(|e| e.title.to_lowercase().contains(&q)).map(|e| e.id.clone()).ok_or_else(|| AppError::not_found("Такого события в календаре нет"))
}

async fn calendar_manage(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: CalArgs = parse(args)?;
    let p = ctx.app.state::<Arc<Planner>>().inner().clone();
    match a.action.as_str() {
        "list" => {
            let from = day(a.from.as_deref().or(a.date.as_deref()))?;
            let to = match a.to.as_deref() {
                Some(t) => day(Some(t))? + CDur::days(1),
                None => from + CDur::days(1),
            };
            let evs = p.list(&from.format("%Y-%m-%d").to_string(), &to.format("%Y-%m-%d").to_string())?;
            Ok(ToolOutput::ok(format!("Событий: {}", evs.len()), json!({ "events": evs })))
        }
        "create" => {
            let start = a.start.ok_or_else(|| AppError::invalid("Нужно начало: ГГГГ-ММ-ДДTЧЧ:ММ или ГГГГ-ММ-ДД"))?;
            let made = p.create(EventInput {
                title: a.title.unwrap_or_default(),
                start,
                end: a.end,
                all_day: a.all_day,
                category: a.category,
                color: a.color,
                notes: a.notes,
                repeat: a.repeat,
                ..Default::default()
            })?;
            let first = made.first().cloned();
            let summary = match &first {
                Some(e) if made.len() > 1 => format!("В календаре: «{}», {} повторов с {}", e.title, made.len(), e.start),
                Some(e) => format!("В календаре: «{}» {} – {}", e.title, e.start, e.end),
                None => "Событие не создано".into(),
            };
            Ok(ToolOutput::ok(summary, json!({ "created": made, "verified": first.is_some() })))
        }
        "update" | "move" => {
            let by_title = a.id.is_none();
            let id = resolve(&p, a.id.as_deref(), a.title.as_deref())?;
            let e = p.update(
                &id,
                EventPatch {
                    title: if by_title { None } else { a.title },
                    start: a.start,
                    end: a.end,
                    all_day: a.all_day,
                    category: a.category,
                    color: a.color,
                    notes: a.notes,
                    ..Default::default()
                },
            )?;
            Ok(ToolOutput::ok(format!("Изменено: «{}» {} – {}", e.title, e.start, e.end), json!({ "event": e, "verified": true })))
        }
        "delete" => {
            let id = resolve(&p, a.id.as_deref(), a.title.as_deref())?;
            let n = p.delete(&id, false)?;
            Ok(ToolOutput::ok("Событие удалено", json!({ "deleted": n })))
        }
        "suggest" => {
            let d = day(a.date.as_deref())?;
            let blocks = crate::planner::suggest::suggest(&ctx.app, &p, d, Local::now().naive_local(), a.wishes.as_deref()).await?;
            Ok(ToolOutput::ok(
                format!("Предложено блоков: {}. Их ещё нет в календаре: поставьте выбранные одним вызовом plan.", blocks.len()),
                json!({ "blocks": blocks }),
            ))
        }
        "free" => {
            let from = day(a.date.as_deref().or(a.from.as_deref()))?;
            let now = Local::now().naive_local();
            let mut days = Vec::new();
            for i in 0..a.days.unwrap_or(1).clamp(1, 7) {
                let d = from + CDur::days(i as i64);
                let c = crate::planner::suggest::context(&p, d, now)?;
                let free: Vec<String> = crate::planner::suggest::free_slots(&c).iter().map(|(s, e)| format!("{}-{}", s.format("%H:%M"), e.format("%H:%M"))).collect();
                let events = p.day(d)?;
                days.push(json!({ "date": d.format("%Y-%m-%d").to_string(), "weekday": crate::planner::suggest::weekday_ru(d), "free": free, "events": events }));
            }
            Ok(ToolOutput::ok(format!("Свободное время на {} дн.", days.len()), json!({ "days": days })))
        }
        "plan" => {
            let blocks = a.blocks.unwrap_or_default();
            if blocks.is_empty() {
                return Err(AppError::invalid("Нужен список blocks: title, start, end (ГГГГ-ММ-ДДTЧЧ:ММ), category"));
            }
            let (ok, bad) = crate::planner::suggest::check_blocks(blocks, |d| crate::planner::suggest::busy_of(&p, d));
            let mut created = Vec::new();
            for b in ok {
                created.extend(p.create(EventInput {
                    title: b.title,
                    start: b.start,
                    end: Some(b.end),
                    category: b.category,
                    notes: b.notes,
                    ..Default::default()
                })?);
            }
            let skipped: Vec<Value> = bad.iter().map(|(b, why)| json!({ "title": b.title, "start": b.start, "end": b.end, "why": why })).collect();
            let summary = if skipped.is_empty() {
                format!("В календаре блоков: {}", created.len())
            } else {
                format!("В календаре блоков: {}, не поставлено: {} (см. skipped)", created.len(), skipped.len())
            };
            Ok(ToolOutput { ok: !created.is_empty(), summary, data: json!({ "created": created, "skipped": skipped, "verified": !created.is_empty() }), images: vec![] })
        }
        "open" => {
            crate::ipc_planner::open_window(&ctx.app, a.date.clone())?;
            Ok(ToolOutput::ok("Планировщик открыт", json!({ "verified": true })))
        }
        other => Err(AppError::invalid(format!("Неизвестное действие calendar_manage: {other}"))),
    }
}

async fn growth(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let m = ctx.app.state::<Arc<Mentor>>().inner().clone();
    if str_arg(&args, "action") == "reset" {
        let screen = Some(str_arg(&args, "screen")).filter(|s| !s.is_empty()).map(str::to_string);
        let summary = match screen.as_deref() {
            Some("feed") => "Открыта лента знаний",
            Some("games") => "Открыта игра на минуту",
            Some("workout") => "Открыта разминка на три минуты",
            Some("breath") => "Открыта минута дыхания",
            Some("step") => "Открыт шаг по проекту: выбрать дело и запустить фокус",
            _ => "Открыта «Перезагрузка»: лента, игра, разминка, шаг по проекту",
        };
        m.voluntary(screen)?;
        return Ok(ToolOutput::ok(summary, json!({ "verified": true })));
    }
    let s = m.state()?;
    let summary = format!(
        "Уровень {} ({}), серия {} дн., сегодня {} XP; короткие видео {} из {} мин",
        s.level,
        s.level_name,
        s.streak,
        s.today_xp,
        s.short.used_sec / 60,
        s.short.limit_sec / 60
    );
    Ok(ToolOutput::ok(summary, serde_json::to_value(&s).unwrap_or(Value::Null)))
}

fn calendar_risk(a: &Value) -> super::Risk {
    if str_arg(a, "action") == "delete" {
        super::Risk::Moderate
    } else {
        super::Risk::Low
    }
}

fn calendar_title(a: &Value) -> String {
    match str_arg(a, "action") {
        "create" => format!("В календарь: «{}»", str_arg(a, "title")),
        "update" | "move" => "Изменить событие".into(),
        "delete" => "Удалить событие".into(),
        "suggest" => "Спланировать день".into(),
        "free" => "Свободное время".into(),
        "plan" => format!("В календарь: план, блоков {}", a.get("blocks").and_then(Value::as_array).map_or(0, Vec::len)),
        "open" => "Открыть планировщик".into(),
        _ => "Календарь".into(),
    }
}

fn growth_title(a: &Value) -> String {
    if str_arg(a, "action") == "reset" {
        "Открыть «Перезагрузку»".into()
    } else {
        "Прогресс роста".into()
    }
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "calendar_manage",
            description: "The user's calendar (Google-Calendar-style planner, synced with Google when connected). \
                `list` events (`date` or `from`/`to` as YYYY-MM-DD), `create` one event (title, start 'YYYY-MM-DDTHH:MM' or 'YYYY-MM-DD' for all-day, \
                end, category work|study|sport|project|personal, optional repeat none|daily|weekdays|weekly), `update`/`move` (id or title + new fields), \
                `delete` (id or title), `free` (free time and events for `date`, `days` up to 7), \
                `suggest` (a ready plan for one day's free time; pass the user's own words about the day as `wishes`; returns blocks, does not create), \
                `plan` (put several blocks into the calendar at once: `blocks` [{title, start, end, category, notes}]; blocks that clash are skipped with the reason), \
                `open` the planner window. Planning a day or a week with the user: you plan it yourself from what they said, their tasks and goals; \
                look at `free` first, then put the agreed blocks in with one `plan` call.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["list", "create", "update", "move", "delete", "free", "suggest", "plan", "open"] },
                    "id": { "type": "string" },
                    "title": { "type": "string" },
                    "start": { "type": "string" },
                    "end": { "type": "string" },
                    "all_day": { "type": "boolean" },
                    "category": { "type": "string", "enum": ["work", "study", "sport", "project", "personal"] },
                    "color": { "type": "string" },
                    "notes": { "type": "string" },
                    "repeat": { "type": "string", "enum": ["none", "daily", "weekdays", "weekly"] },
                    "from": { "type": "string" },
                    "to": { "type": "string" },
                    "date": { "type": "string" },
                    "days": { "type": "integer", "minimum": 1, "maximum": 7 },
                    "wishes": { "type": "string" },
                    "blocks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "start": { "type": "string" },
                                "end": { "type": "string" },
                                "category": { "type": "string", "enum": ["work", "study", "sport", "project", "personal"] },
                                "notes": { "type": "string" }
                            },
                            "required": ["title", "start", "end"]
                        }
                    }
                }),
                &["action"],
            ),
            risk: calendar_risk,
            title: calendar_title,
            timeout: Duration::from_secs(60),
            parallel_safe: false,
            handler: handler(calendar_manage),
        },
        Tool {
            name: "growth",
            description: "The mentor: `status` (level, XP, streak, today's short-video minutes used and left, strict mode) or `reset` \
                (open «Перезагрузка»: card feed, game, workout, project step) when the user wants a healthy break or is stuck. \
                `screen` opens one activity straight away: «давай разминку» → workout, «подыши со мной» → breath.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["status", "reset"] },
                    "screen": { "type": "string", "enum": ["feed", "games", "workout", "breath", "step"] }
                }),
                &["action"],
            ),
            risk: low,
            title: growth_title,
            timeout: Duration::from_secs(10),
            parallel_safe: true,
            handler: handler(growth),
        },
    ]
}
