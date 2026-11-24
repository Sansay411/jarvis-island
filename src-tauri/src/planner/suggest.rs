//! «Спланировать день»: the chat model plans the free time between the
//! user's events the way a personal assistant would: the user's own wishes
//! for the day first («хочу закончить отчёт и сходить в зал»), then open
//! tasks, then what they grow in. The fixed plan below is only for when no
//! model answers. The placement rules always decide where blocks may go, so
//! a proposal never overlaps the user's own events.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

use super::{parse_when, Planner, When};
use crate::ai::types::{ChatMessage, Role};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub title: String,
    /// `YYYY-MM-DDTHH:MM`
    pub start: String,
    pub end: String,
    pub category: String,
    /// One line on why this block is here.
    pub why: String,
}

#[derive(Debug, Clone)]
pub struct DayContext {
    pub date: NaiveDate,
    /// Blocks start no earlier than this (now, on today).
    pub not_before: NaiveDateTime,
    pub busy: Vec<(NaiveDateTime, NaiveDateTime)>,
    pub work_start: NaiveTime,
    pub work_end: NaiveTime,
    pub tracks: Vec<String>,
    pub project: Option<String>,
    pub tasks: Vec<String>,
}

type Slot = (NaiveDateTime, NaiveDateTime);

fn at(d: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
    d.and_hms_opt(h, m, 0).expect("valid time")
}

/// Up to the next quarter hour.
fn ceil_quarter(t: NaiveDateTime) -> NaiveDateTime {
    let extra = (15 - t.minute() % 15) % 15;
    let t = t.with_second(0).and_then(|t| t.with_nanosecond(0)).unwrap_or(t);
    if extra == 0 { t } else { t + Duration::minutes(extra as i64) }
}

/// Free time of the day between 07:00 and 22:30, minus the busy spans.
pub fn free_slots(ctx: &DayContext) -> Vec<Slot> {
    let mut start = at(ctx.date, 7, 0).max(ceil_quarter(ctx.not_before));
    let end = at(ctx.date, 22, 30);
    let mut busy = ctx.busy.clone();
    busy.sort();
    let mut out = Vec::new();
    for (bs, be) in busy {
        if be <= start {
            continue;
        }
        if bs >= end {
            break;
        }
        if bs > start {
            out.push((start, bs.min(end)));
        }
        start = start.max(be);
    }
    if start < end {
        out.push((start, end));
    }
    out.into_iter().filter(|(s, e)| *e - *s >= Duration::minutes(15)).collect()
}

/// Take `len` minutes from the first slot that fits inside `[from, to)`.
fn place(slots: &mut Vec<Slot>, len: i64, from: NaiveDateTime, to: NaiveDateTime) -> Option<Slot> {
    let len = Duration::minutes(len);
    for i in 0..slots.len() {
        let (s, e) = slots[i];
        let start = s.max(from);
        if start + len <= e.min(to) {
            let block = (start, start + len);
            // A ten-minute breather after each placed block.
            let after = block.1 + Duration::minutes(10);
            let mut rest = Vec::new();
            if start - s >= Duration::minutes(15) {
                rest.push((s, start));
            }
            if e - after >= Duration::minutes(15) {
                rest.push((after, e));
            }
            slots.splice(i..=i, rest);
            return Some(block);
        }
    }
    None
}

fn fmt(t: NaiveDateTime) -> String {
    When::At(t).format()
}

/// The plan without a model: fixed blocks in their natural time of day.
pub fn heuristic(ctx: &DayContext) -> Vec<Suggestion> {
    let d = ctx.date;
    let mut slots = free_slots(ctx);
    let work_s = d.and_time(ctx.work_start);
    let work_e = d.and_time(ctx.work_end).max(work_s + Duration::hours(1));
    let has = |t: &str| ctx.tracks.iter().any(|x| x == t);
    let mut want: Vec<(String, i64, &str, NaiveDateTime, NaiveDateTime, &str)> = Vec::new();
    if has("projects") {
        let name = ctx.project.clone().unwrap_or_else(|| "главный проект".into());
        want.push((format!("Глубокая работа: {name}"), 90, "project", work_s, at(d, 13, 0), "Утром голова свежая — самое трудное сейчас."));
    }
    if has("code") {
        want.push(("Программирование: практика".into(), 45, "study", work_s, work_e, "Новый навык закрепляется руками, а не роликами."));
    }
    if has("english") {
        want.push(("Английский: 20 минут".into(), 20, "study", at(d, 12, 0), at(d, 21, 0), "Короткий ежедневный повтор держит слова в памяти."));
    }
    if has("body") {
        want.push(("Тренировка".into(), 45, "sport", at(d, 17, 0), at(d, 21, 0), "Вечерняя нагрузка снимает накопленную за день усталость."));
    }
    let mut out = Vec::new();
    for (title, len, cat, from, to, why) in want {
        let block = place(&mut slots, len, from, to).or_else(|| place(&mut slots, len, at(d, 7, 0), at(d, 22, 30)));
        if let Some((s, e)) = block {
            out.push(Suggestion { title, start: fmt(s), end: fmt(e), category: cat.into(), why: why.into() });
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start));
    out
}

/// Keep the model's blocks that land in free time and don't overlap.
pub fn validate(ctx: &DayContext, raw: Vec<Suggestion>) -> Vec<Suggestion> {
    let free = free_slots(ctx);
    let mut taken: Vec<Slot> = Vec::new();
    let mut out = Vec::new();
    for mut s in raw {
        let (Some(When::At(a)), Some(When::At(b))) = (parse_when(&s.start), parse_when(&s.end)) else { continue };
        if a.date() != ctx.date || b <= a || b - a < Duration::minutes(10) || b - a > Duration::hours(4) {
            continue;
        }
        let inside = free.iter().any(|(fs, fe)| a >= *fs && b <= *fe);
        let clash = taken.iter().any(|(ts, te)| a < *te && b > *ts);
        if !inside || clash || s.title.trim().is_empty() {
            continue;
        }
        if !super::CATEGORIES.iter().any(|(id, _, _)| *id == s.category) || s.category == "google" {
            s.category = "personal".into();
        }
        s.title = s.title.trim().chars().take(80).collect();
        s.start = fmt(a);
        s.end = fmt(b);
        taken.push((a, b));
        out.push(s);
    }
    out.sort_by(|a, b| a.start.cmp(&b.start));
    out
}

/// The first `{…}` JSON object in a model's reply.
fn json_in(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok()
}

fn hm(s: &str, fallback: (u32, u32)) -> NaiveTime {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").unwrap_or_else(|_| NaiveTime::from_hms_opt(fallback.0, fallback.1, 0).expect("valid"))
}

pub fn context(planner: &Planner, date: NaiveDate, now: NaiveDateTime) -> AppResult<DayContext> {
    let settings = planner.store().settings();
    let busy = planner
        .day(date)?
        .into_iter()
        .filter(|e| !e.all_day)
        .filter_map(|e| match (parse_when(&e.start), parse_when(&e.end)) {
            (Some(When::At(a)), Some(When::At(b))) => Some((a, b)),
            _ => None,
        })
        .collect();
    let project = settings
        .agent
        .project_roots
        .iter()
        .filter_map(|p| std::path::Path::new(p.trim()).file_name().map(|n| n.to_string_lossy().to_string()))
        .find(|n| !n.is_empty());
    Ok(DayContext {
        date,
        not_before: if date == now.date() { now } else { at(date, 0, 0) },
        busy,
        work_start: hm(&settings.planner.work_start, (9, 0)),
        work_end: hm(&settings.planner.work_end, (18, 0)),
        tracks: settings.mentor.tracks.clone(),
        project,
        tasks: Vec::new(),
    })
}

/// Propose blocks for `date`: the model's plan when it answers, the fixed
/// plan otherwise. `wishes`: what the user said they want from the day.
pub async fn suggest(app: &AppHandle, planner: &Planner, date: NaiveDate, now: NaiveDateTime, wishes: Option<&str>) -> AppResult<Vec<Suggestion>> {
    let mut ctx = context(planner, date, now)?;
    if let Some(hub) = app.try_state::<std::sync::Arc<crate::hub::Hub>>() {
        let day = date.format("%Y-%m-%d").to_string();
        ctx.tasks = hub
            .tasks_list()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !t.completed && t.scheduled_date.as_deref().is_none_or(|d| d == day))
            .map(|t| t.title)
            .take(8)
            .collect();
    }
    let fallback = heuristic(&ctx);
    let state = app.state::<AppState>();
    let settings = state.store.settings();
    let Ok(client) = crate::ai::client_from_settings(&settings, &state.store) else {
        return Ok(fallback);
    };
    let free: Vec<String> = free_slots(&ctx).iter().map(|(a, b)| format!("{}–{}", a.format("%H:%M"), b.format("%H:%M"))).collect();
    let busy: Vec<String> = planner
        .day(date)?
        .iter()
        .filter(|e| !e.all_day)
        .map(|e| format!("{}–{} {}", &e.start[11..], &e.end.get(11..).unwrap_or(""), e.title))
        .collect();
    let tracks: Vec<&str> = ctx
        .tracks
        .iter()
        .map(|t| match t.as_str() {
            "code" => "программирование",
            "english" => "английский",
            "body" => "спорт и тело",
            "projects" => "свои проекты",
            other => other,
        })
        .collect();
    let wishes = wishes.map(str::trim).filter(|w| !w.is_empty());
    let prompt = format!(
        "Дата: {date} ({weekday}). Сейчас {now}.\nУже в календаре: {busy}\nСвободные окна: {free}\nРабочие часы: {ws}-{we}.\n\
         Цели роста: {tracks}. Главный проект: {project}. Открытые задачи: {tasks}.\nПожелания на этот день: {wishes}\n\n\
         Спланируй этот день сам, как хороший личный ассистент, а не по шаблону. Важнее всего пожелания человека, \
         затем открытые задачи, затем цели роста. Думай об энергии: трудное на свежую голову, лёгкое после обеда; \
         не забивай каждую минуту, оставляй паузы, время на еду и отдых. Столько блоков, сколько разумно для этого дня \
         (бывает и один). Длительность по смыслу дела. Каждому блоку конкретное короткое название по задаче или пожеланию, \
         а не общее «Работа». Только в свободных окнах, без пересечений. why: одна фраза, почему блок именно тогда.\n\
         Ответ только JSON: {{\"blocks\":[{{\"title\":\"…\",\"start\":\"HH:MM\",\"end\":\"HH:MM\",\
         \"category\":\"work|study|sport|project|personal\",\"why\":\"…\"}}]}}",
        weekday = weekday_ru(date),
        now = now.format("%H:%M"),
        busy = if busy.is_empty() { "ничего".into() } else { busy.join("; ") },
        free = if free.is_empty() { "нет".into() } else { free.join(", ") },
        ws = ctx.work_start.format("%H:%M"),
        we = ctx.work_end.format("%H:%M"),
        tracks = tracks.join(", "),
        project = ctx.project.as_deref().unwrap_or("не указан"),
        tasks = if ctx.tasks.is_empty() { "нет".into() } else { ctx.tasks.join("; ") },
        wishes = wishes.unwrap_or("не названы, исходи из задач и целей"),
    );
    let msgs = [
        ChatMessage::text(Role::System, "Ты планировщик дня внутри ассистента Джарвис. Пишешь по-русски, отвечаешь строго JSON без пояснений."),
        ChatMessage::text(Role::User, prompt),
    ];
    let cancel = CancellationToken::new();
    let reply = tokio::time::timeout(std::time::Duration::from_secs(40), client.complete(&msgs, &[], &cancel, Some(900), |_| {})).await;
    let Ok(Ok(done)) = reply else { return Ok(fallback) };
    let Some(v) = json_in(&done.message.joined_text()) else { return Ok(fallback) };
    let raw: Vec<Suggestion> = v
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| {
                    let s = b.get("start")?.as_str()?;
                    let e = b.get("end")?.as_str()?;
                    Some(Suggestion {
                        title: b.get("title")?.as_str()?.to_string(),
                        start: format!("{date}T{}", s.trim()),
                        end: format!("{date}T{}", e.trim()),
                        category: b.get("category").and_then(Value::as_str).unwrap_or("personal").to_string(),
                        why: b.get("why").and_then(Value::as_str).unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let valid = validate(&ctx, raw);
    Ok(if valid.is_empty() { fallback } else { valid })
}

/// A block the agent itself decided on (`calendar_manage` → `plan`).
#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    pub title: String,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Which of the agent's blocks may go into the calendar as they are: on one
/// day, between ten minutes and six hours, clear of the timed events already
/// there (`busy`) and of each other. The rest come back with the reason.
pub fn check_blocks(blocks: Vec<Block>, busy: impl Fn(NaiveDate) -> Vec<Slot>) -> (Vec<Block>, Vec<(Block, &'static str)>) {
    let mut ok: Vec<(Block, Slot)> = Vec::new();
    let mut bad = Vec::new();
    for b in blocks {
        let (Some(When::At(a)), Some(When::At(e))) = (parse_when(&b.start), parse_when(&b.end)) else {
            bad.push((b, "время ожидается как ГГГГ-ММ-ДДTЧЧ:ММ"));
            continue;
        };
        let why = if b.title.trim().is_empty() {
            Some("нет названия")
        } else if a.date() != e.date() || e <= a {
            Some("конец должен быть позже начала в тот же день")
        } else if e - a < Duration::minutes(10) || e - a > Duration::hours(6) {
            Some("блок от 10 минут до 6 часов")
        } else if busy(a.date()).iter().any(|(s, f)| a < *f && e > *s) {
            Some("пересекается с событием в календаре")
        } else if ok.iter().any(|(_, (s, f))| a < *f && e > *s) {
            Some("пересекается с другим блоком плана")
        } else {
            None
        };
        match why {
            Some(w) => bad.push((b, w)),
            None => ok.push((b, (a, e))),
        }
    }
    ok.sort_by_key(|(_, (a, _))| *a);
    (ok.into_iter().map(|(b, _)| b).collect(), bad)
}

/// The timed events of a day as spans.
pub fn busy_of(planner: &Planner, date: NaiveDate) -> Vec<Slot> {
    planner
        .day(date)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.all_day)
        .filter_map(|e| match (parse_when(&e.start), parse_when(&e.end)) {
            (Some(When::At(a)), Some(When::At(b))) => Some((a, b)),
            _ => None,
        })
        .collect()
}

pub fn weekday_ru(d: NaiveDate) -> &'static str {
    use chrono::Datelike;
    ["понедельник", "вторник", "среда", "четверг", "пятница", "суббота", "воскресенье"][d.weekday().num_days_from_monday() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
    }

    fn ctx(busy: &[(&str, &str)], not_before: &str) -> DayContext {
        DayContext {
            date: NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(),
            not_before: t(not_before),
            busy: busy.iter().map(|(a, b)| (t(a), t(b))).collect(),
            work_start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            work_end: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            tracks: vec!["code".into(), "english".into(), "body".into(), "projects".into()],
            project: Some("jarvis-island".into()),
            tasks: vec![],
        }
    }

    #[test]
    fn free_time_skips_busy_spans_and_the_past() {
        let c = ctx(&[("2026-11-23T10:00", "2026-11-23T11:00"), ("2026-11-23T10:30", "2026-11-23T12:00")], "2026-11-23T08:07");
        let free = free_slots(&c);
        assert_eq!(free[0], (t("2026-11-23T08:15"), t("2026-11-23T10:00")));
        assert_eq!(free[1], (t("2026-11-23T12:00"), t("2026-11-23T22:30")));
    }

    #[test]
    fn heuristic_plan_fits_around_events() {
        let c = ctx(&[("2026-11-23T09:00", "2026-11-23T10:00"), ("2026-11-23T17:00", "2026-11-23T18:00")], "2026-11-23T07:00");
        let plan = heuristic(&c);
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0].title, "Глубокая работа: jarvis-island");
        assert_eq!((plan[0].start.as_str(), plan[0].end.as_str()), ("2026-11-23T10:00", "2026-11-23T11:30"));
        let sport = plan.iter().find(|s| s.category == "sport").unwrap();
        assert_eq!(sport.start, "2026-11-23T18:00");
        // Nothing overlaps the user's events or each other.
        let spans: Vec<_> = plan.iter().map(|s| (t(&s.start), t(&s.end))).collect();
        for (i, a) in spans.iter().enumerate() {
            for b in &spans[i + 1..] {
                assert!(a.1 <= b.0 || b.1 <= a.0);
            }
            for (bs, be) in &c.busy {
                assert!(a.1 <= *bs || *be <= a.0);
            }
        }
    }

    #[test]
    fn late_in_the_day_only_what_fits() {
        let plan = heuristic(&ctx(&[], "2026-11-23T21:40"));
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].start, "2026-11-23T21:45");
    }

    #[test]
    fn agent_blocks_go_in_only_when_they_fit() {
        let busy = |_: NaiveDate| vec![(t("2026-11-23T10:00"), t("2026-11-23T11:00"))];
        let b = |title: &str, a: &str, e: &str| Block { title: title.into(), start: a.into(), end: e.into(), category: None, notes: None };
        let (ok, bad) = check_blocks(
            vec![
                b("Зал", "2026-11-23T19:00", "2026-11-23T20:00"),
                b("Отчёт", "2026-11-23T08:30", "2026-11-23T10:00"),
                b("Поверх встречи", "2026-11-23T10:30", "2026-11-23T11:30"),
                b("Внахлёст с отчётом", "2026-11-23T09:30", "2026-11-23T09:50"),
                b("Ночь", "2026-11-23T23:00", "2026-11-24T01:00"),
                b("", "2026-11-23T13:00", "2026-11-23T14:00"),
            ],
            busy,
        );
        assert_eq!(ok.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(), ["Отчёт", "Зал"]);
        assert_eq!(bad.len(), 4);
        assert_eq!(bad[0].1, "пересекается с событием в календаре");
    }

    #[test]
    fn model_blocks_are_checked() {
        let c = ctx(&[("2026-11-23T10:00", "2026-11-23T11:00")], "2026-11-23T07:00");
        let s = |title: &str, a: &str, b: &str, cat: &str| Suggestion { title: title.into(), start: a.into(), end: b.into(), category: cat.into(), why: String::new() };
        let ok = validate(
            &c,
            vec![
                s("Работа", "2026-11-23T08:00", "2026-11-23T09:30", "project"),
                s("Поверх встречи", "2026-11-23T10:30", "2026-11-23T11:30", "work"),
                s("Внахлёст", "2026-11-23T09:00", "2026-11-23T09:45", "study"),
                s("Зал", "2026-11-23T18:00", "2026-11-23T19:00", "fitness"),
                s("Завтра", "2026-11-24T08:00", "2026-11-24T09:00", "work"),
            ],
        );
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[0].title, "Работа");
        assert_eq!(ok[1].category, "personal");
        assert_eq!(json_in("```json\n{\"blocks\":[]}\n```").unwrap()["blocks"], serde_json::json!([]));
    }
}
