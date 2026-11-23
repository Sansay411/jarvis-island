//! The planner: the user's calendar, laid out like Google Calendar (hour
//! grid, colour per calendar, drag to move and stretch), kept in SQLite and,
//! once the user connects an account, mirrored both ways with Google
//! Calendar ([`google`]). Event times are local wall-clock strings —
//! `YYYY-MM-DDTHH:MM` for timed events and `YYYY-MM-DD` for all-day ones,
//! ends exclusive — so lexical order is time order and range queries are
//! plain string comparisons.

pub mod google;
pub mod suggest;

use std::sync::Arc;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Utc, Weekday};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};
use crate::store::Store;

pub const EVT_CHANGED: &str = "planner:changed";

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS planner_events (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    notes       TEXT NOT NULL DEFAULT '',
    location    TEXT NOT NULL DEFAULT '',
    start_at    TEXT NOT NULL,
    end_at      TEXT NOT NULL,
    all_day     INTEGER NOT NULL DEFAULT 0,
    color       TEXT NOT NULL DEFAULT '',
    category    TEXT NOT NULL DEFAULT 'personal',
    done        INTEGER NOT NULL DEFAULT 0,
    series_id   TEXT,
    origin      TEXT NOT NULL DEFAULT 'local',
    google_id   TEXT,
    recurring   INTEGER NOT NULL DEFAULT 0,
    sync_state  TEXT NOT NULL DEFAULT '',
    deleted     INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS planner_events_range ON planner_events(deleted, start_at);
CREATE INDEX IF NOT EXISTS planner_events_google ON planner_events(google_id);
"#;

/// Google Calendar's event palette: name, hex, the API's `colorId`.
pub const COLORS: &[(&str, &str, u8)] = &[
    ("lavender", "#7986cb", 1),
    ("sage", "#33b679", 2),
    ("grape", "#8e24aa", 3),
    ("flamingo", "#e67c73", 4),
    ("banana", "#f6bf26", 5),
    ("tangerine", "#f4511e", 6),
    ("peacock", "#039be5", 7),
    ("graphite", "#616161", 8),
    ("blueberry", "#3f51b5", 9),
    ("basil", "#0b8043", 10),
    ("tomato", "#d50000", 11),
];

/// «Мои календари»: id, label, default colour. The mentor reads the ids:
/// work, study and project blocks are focus time.
pub const CATEGORIES: &[(&str, &str, &str)] = &[
    ("work", "Работа", "blueberry"),
    ("study", "Учёба", "grape"),
    ("sport", "Спорт", "basil"),
    ("project", "Проекты", "tangerine"),
    ("personal", "Личное", "flamingo"),
    ("google", "Google Календарь", "peacock"),
];

pub fn color_id(name: &str) -> Option<u8> {
    COLORS.iter().find(|(n, _, _)| *n == name).map(|(_, _, id)| *id)
}

pub fn color_by_id(id: u8) -> Option<&'static str> {
    COLORS.iter().find(|(_, _, i)| *i == id).map(|(n, _, _)| *n)
}

pub fn category_color(category: &str) -> &'static str {
    CATEGORIES.iter().find(|(id, _, _)| *id == category).map(|(_, _, c)| *c).unwrap_or("peacock")
}

pub fn category_label(category: &str) -> &'static str {
    CATEGORIES.iter().find(|(id, _, _)| *id == category).map(|(_, l, _)| *l).unwrap_or("Личное")
}

fn normalize_category(c: Option<&str>) -> String {
    let c = c.unwrap_or("personal").trim().to_lowercase();
    if CATEGORIES.iter().any(|(id, _, _)| *id == c) {
        c
    } else {
        "personal".into()
    }
}

fn normalize_color(c: Option<&str>) -> String {
    let c = c.unwrap_or("").trim().to_lowercase();
    if c.is_empty() || color_id(&c).is_some() {
        c
    } else {
        String::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalEvent {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub location: String,
    /// `YYYY-MM-DDTHH:MM` (timed) or `YYYY-MM-DD` (all-day).
    pub start: String,
    /// Exclusive end in the same format.
    pub end: String,
    pub all_day: bool,
    /// Google palette name; empty = the category's colour.
    pub color: String,
    pub category: String,
    pub done: bool,
    pub series_id: Option<String>,
    pub google_id: Option<String>,
    /// An occurrence of a repeating Google event: edits change this one.
    pub recurring: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EventInput {
    pub title: String,
    pub notes: Option<String>,
    pub location: Option<String>,
    pub start: String,
    pub end: Option<String>,
    pub all_day: Option<bool>,
    pub color: Option<String>,
    pub category: Option<String>,
    /// none | daily | weekdays | weekly
    pub repeat: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EventPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub location: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub all_day: Option<bool>,
    pub color: Option<String>,
    pub category: Option<String>,
    pub done: Option<bool>,
}

/// A point on the calendar: a whole day or a minute of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum When {
    Day(NaiveDate),
    At(NaiveDateTime),
}

impl When {
    pub fn format(self) -> String {
        match self {
            When::Day(d) => d.format("%Y-%m-%d").to_string(),
            When::At(t) => t.format("%Y-%m-%dT%H:%M").to_string(),
        }
    }
    pub fn date(self) -> NaiveDate {
        match self {
            When::Day(d) => d,
            When::At(t) => t.date(),
        }
    }
}

/// Parse `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM[:SS]` or `YYYY-MM-DD HH:MM`.
pub fn parse_when(s: &str) -> Option<When> {
    let s = s.trim();
    if s.len() == 10 {
        return NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().map(When::Day);
    }
    let s = s.replacen(' ', "T", 1);
    let head = s.get(..16)?;
    NaiveDateTime::parse_from_str(head, "%Y-%m-%dT%H:%M").ok().map(When::At)
}

/// Bring a start/end pair into shape: all-day ends are exclusive dates after
/// the start; timed events last at least 15 minutes (default 1 hour).
pub fn normalize_span(start: &str, end: Option<&str>, all_day: bool) -> AppResult<(String, String)> {
    let bad = || AppError::invalid(format!("Некорректное время «{start}» (ожидается ГГГГ-ММ-ДДTЧЧ:ММ)"));
    let s = parse_when(start).ok_or_else(bad)?;
    let e = end.and_then(parse_when);
    if all_day {
        let sd = s.date();
        let ed = e.map(When::date).filter(|d| *d > sd).unwrap_or(sd + Duration::days(1));
        return Ok((When::Day(sd).format(), When::Day(ed).format()));
    }
    let st = match s {
        When::At(t) => t,
        When::Day(d) => d.and_hms_opt(9, 0, 0).expect("valid time"),
    };
    let et = match e {
        Some(When::At(t)) if t >= st + Duration::minutes(15) => t,
        Some(When::At(t)) if t > st => st + Duration::minutes(15),
        _ => st + Duration::hours(1),
    };
    Ok((When::At(st).format(), When::At(et).format()))
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

const COLUMNS: &str = "id, title, notes, location, start_at, end_at, all_day, color, category, done, series_id, google_id, recurring, updated_at";

fn row_event(r: &Row<'_>) -> rusqlite::Result<CalEvent> {
    Ok(CalEvent {
        id: r.get(0)?,
        title: r.get(1)?,
        notes: r.get(2)?,
        location: r.get(3)?,
        start: r.get(4)?,
        end: r.get(5)?,
        all_day: r.get::<_, i64>(6)? != 0,
        color: r.get(7)?,
        category: r.get(8)?,
        done: r.get::<_, i64>(9)? != 0,
        series_id: r.get(10)?,
        google_id: r.get(11)?,
        recurring: r.get::<_, i64>(12)? != 0,
        updated_at: r.get(13)?,
    })
}

/// Start dates of a repeating series beginning at `first`.
fn occurrences(first: NaiveDate, repeat: &str) -> Vec<NaiveDate> {
    match repeat {
        "daily" => (0..30).map(|i| first + Duration::days(i)).collect(),
        "weekdays" => (0..42)
            .map(|i| first + Duration::days(i))
            .filter(|d| !matches!(d.weekday(), Weekday::Sat | Weekday::Sun))
            .take(30)
            .collect(),
        "weekly" => (0..12).map(|i| first + Duration::weeks(i)).collect(),
        _ => vec![first],
    }
}

fn shift(when: &str, days: i64) -> String {
    match parse_when(when) {
        Some(When::Day(d)) => When::Day(d + Duration::days(days)).format(),
        Some(When::At(t)) => When::At(t + Duration::days(days)).format(),
        None => when.to_string(),
    }
}

pub struct Planner {
    app: Option<AppHandle>,
    store: Arc<Store>,
    pub google: google::Google,
}

impl Planner {
    pub fn new(app: AppHandle, store: Arc<Store>) -> Arc<Self> {
        Arc::new(Planner { app: Some(app), store, google: google::Google::default() })
    }

    pub fn new_headless(store: Arc<Store>) -> Arc<Self> {
        Arc::new(Planner { app: None, store, google: google::Google::default() })
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn app(&self) -> Option<&AppHandle> {
        self.app.as_ref()
    }

    /// Tell every window the calendar changed and nudge the Google sync.
    pub fn changed(&self) {
        if let Some(app) = &self.app {
            let _ = app.emit(EVT_CHANGED, ());
        }
        self.google.poke();
    }

    /// Events overlapping the dates `[from, to)` (`YYYY-MM-DD`), by start.
    pub fn list(&self, from: &str, to: &str) -> AppResult<Vec<CalEvent>> {
        self.store.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM planner_events
                 WHERE deleted = 0 AND start_at < ?2 AND end_at > ?1
                 ORDER BY start_at, all_day DESC, end_at DESC, title"
            ))?;
            let rows = stmt.query_map(params![from, to], row_event)?;
            Ok(rows.filter_map(Result::ok).collect())
        })
    }

    /// The events of one day, all-day first.
    pub fn day(&self, date: NaiveDate) -> AppResult<Vec<CalEvent>> {
        let next = date + Duration::days(1);
        let mut v = self.list(&date.format("%Y-%m-%d").to_string(), &next.format("%Y-%m-%d").to_string())?;
        v.sort_by(|a, b| b.all_day.cmp(&a.all_day).then(a.start.cmp(&b.start)));
        Ok(v)
    }

    /// Timed events running at `now` (local wall time).
    pub fn current(&self, now: NaiveDateTime) -> AppResult<Vec<CalEvent>> {
        let at = When::At(now).format();
        Ok(self.day(now.date())?.into_iter().filter(|e| !e.all_day && e.start <= at && e.end > at).collect())
    }

    pub fn get(&self, id: &str) -> AppResult<CalEvent> {
        self.store
            .with_conn(|conn| {
                Ok(conn
                    .query_row(&format!("SELECT {COLUMNS} FROM planner_events WHERE id = ?1 AND deleted = 0"), params![id], row_event)
                    .optional()?)
            })?
            .ok_or_else(|| AppError::not_found("Событие не найдено"))
    }

    /// Create an event (a series of them when `repeat` is set).
    pub fn create(&self, input: EventInput) -> AppResult<Vec<CalEvent>> {
        let title = input.title.trim().to_string();
        let title = if title.is_empty() { "(Без названия)".to_string() } else { title };
        let all_day = input.all_day.unwrap_or_else(|| input.start.trim().len() == 10);
        let (start, end) = normalize_span(&input.start, input.end.as_deref(), all_day)?;
        let category = normalize_category(input.category.as_deref());
        let color = normalize_color(input.color.as_deref());
        let repeat = input.repeat.as_deref().unwrap_or("none").trim().to_lowercase();
        let first = parse_when(&start).map(When::date).expect("normalized");
        let dates = occurrences(first, &repeat);
        let series = (dates.len() > 1).then(|| uuid::Uuid::new_v4().to_string());
        let now = now_rfc3339();
        let notes = input.notes.unwrap_or_default().trim().to_string();
        let location = input.location.unwrap_or_default().trim().to_string();
        let mut ids = Vec::with_capacity(dates.len());
        self.store.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for d in &dates {
                let offset = (*d - first).num_days();
                let id = uuid::Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO planner_events(id, title, notes, location, start_at, end_at, all_day, color, category, series_id, origin, created_at, updated_at)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'local', ?11, ?11)",
                    params![id, title, notes, location, shift(&start, offset), shift(&end, offset), all_day as i64, color, category, series, now],
                )?;
                ids.push(id);
            }
            tx.commit()?;
            Ok(())
        })?;
        self.changed();
        ids.iter().map(|id| self.get(id)).collect()
    }

    pub fn update(&self, id: &str, patch: EventPatch) -> AppResult<CalEvent> {
        let cur = self.get(id)?;
        let all_day = patch.all_day.unwrap_or(cur.all_day);
        let start = patch.start.clone().unwrap_or_else(|| cur.start.clone());
        let end = patch.end.clone().or_else(|| {
            // Switching all-day ↔ timed keeps the date, not the old end.
            (all_day == cur.all_day).then(|| cur.end.clone())
        });
        let (start, end) = normalize_span(&start, end.as_deref(), all_day)?;
        let title = patch.title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or(cur.title);
        let notes = patch.notes.map(|n| n.trim().to_string()).unwrap_or(cur.notes);
        let location = patch.location.map(|l| l.trim().to_string()).unwrap_or(cur.location);
        let color = patch.color.map(|c| normalize_color(Some(&c))).unwrap_or(cur.color);
        let category = patch.category.map(|c| normalize_category(Some(&c))).unwrap_or(cur.category);
        let done = patch.done.unwrap_or(cur.done);
        self.store.with_conn(|conn| {
            conn.execute(
                "UPDATE planner_events SET title = ?2, notes = ?3, location = ?4, start_at = ?5, end_at = ?6, all_day = ?7,
                 color = ?8, category = ?9, done = ?10, updated_at = ?11,
                 sync_state = CASE WHEN google_id IS NULL THEN sync_state ELSE 'push' END
                 WHERE id = ?1",
                params![id, title, notes, location, start, end, all_day as i64, color, category, done as i64, now_rfc3339()],
            )?;
            Ok(())
        })?;
        self.changed();
        self.get(id)
    }

    /// Delete an event; `following` also removes the later events of its series.
    pub fn delete(&self, id: &str, following: bool) -> AppResult<usize> {
        let cur = self.get(id)?;
        let n = self.store.with_conn(|conn| {
            let ids: Vec<String> = match (&cur.series_id, following) {
                (Some(series), true) => {
                    let mut stmt = conn.prepare("SELECT id FROM planner_events WHERE series_id = ?1 AND start_at >= ?2 AND deleted = 0")?;
                    let rows = stmt.query_map(params![series, cur.start], |r| r.get::<_, String>(0))?;
                    rows.filter_map(Result::ok).collect()
                }
                _ => vec![cur.id.clone()],
            };
            let tx = conn.unchecked_transaction()?;
            for id in &ids {
                // Events Google knows about wait for the remote delete; the
                // rest go now.
                tx.execute(
                    "UPDATE planner_events SET deleted = 1, sync_state = 'delete', updated_at = ?2 WHERE id = ?1 AND google_id IS NOT NULL",
                    params![id, now_rfc3339()],
                )?;
                tx.execute("DELETE FROM planner_events WHERE id = ?1 AND google_id IS NULL", params![id])?;
            }
            tx.commit()?;
            Ok(ids.len())
        })?;
        self.changed();
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planner() -> Arc<Planner> {
        Planner::new_headless(Arc::new(Store::in_memory().unwrap()))
    }

    fn input(title: &str, start: &str, end: Option<&str>) -> EventInput {
        EventInput { title: title.into(), start: start.into(), end: end.map(Into::into), ..Default::default() }
    }

    #[test]
    fn spans_are_normalized() {
        assert_eq!(normalize_span("2026-11-23T15:00", Some("2026-11-23T16:30"), false).unwrap(), ("2026-11-23T15:00".into(), "2026-11-23T16:30".into()));
        // No end, or an end before the start: one hour.
        assert_eq!(normalize_span("2026-11-23 15:00", None, false).unwrap().1, "2026-11-23T16:00");
        assert_eq!(normalize_span("2026-11-23T15:00", Some("2026-11-23T14:00"), false).unwrap().1, "2026-11-23T16:00");
        // Too short: 15 minutes.
        assert_eq!(normalize_span("2026-11-23T15:00", Some("2026-11-23T15:05"), false).unwrap().1, "2026-11-23T15:15");
        // Seconds are dropped; midnight crossing works.
        assert_eq!(normalize_span("2026-11-23T23:30:00", Some("2026-11-24T00:30"), false).unwrap(), ("2026-11-23T23:30".into(), "2026-11-24T00:30".into()));
        // All-day: exclusive end, at least one day.
        assert_eq!(normalize_span("2026-11-23", None, true).unwrap(), ("2026-11-23".into(), "2026-11-24".into()));
        assert_eq!(normalize_span("2026-11-23T10:00", Some("2026-11-26"), true).unwrap(), ("2026-11-23".into(), "2026-11-26".into()));
        assert!(normalize_span("завтра", None, false).is_err());
    }

    #[test]
    fn create_list_update_delete() {
        let p = planner();
        let ev = p.create(input("Созвон", "2026-11-23T15:00", Some("2026-11-23T16:00"))).unwrap().remove(0);
        assert_eq!(ev.category, "personal");
        assert!(!ev.all_day);
        // Range overlap: the event is on the 23rd only.
        assert_eq!(p.list("2026-11-23", "2026-11-24").unwrap().len(), 1);
        assert_eq!(p.list("2026-11-24", "2026-11-25").unwrap().len(), 0);
        assert_eq!(p.list("2026-11-16", "2026-11-23").unwrap().len(), 0);

        let moved = p
            .update(&ev.id, EventPatch { start: Some("2026-11-24T10:00".into()), end: Some("2026-11-24T11:30".into()), color: Some("tomato".into()), ..Default::default() })
            .unwrap();
        assert_eq!(moved.start, "2026-11-24T10:00");
        assert_eq!(moved.end, "2026-11-24T11:30");
        assert_eq!(moved.color, "tomato");
        // Unknown colours fall back to the calendar's.
        let plain = p.update(&ev.id, EventPatch { color: Some("rainbow".into()), ..Default::default() }).unwrap();
        assert_eq!(plain.color, "");

        // To all-day and back keeps the date.
        let day = p.update(&ev.id, EventPatch { all_day: Some(true), ..Default::default() }).unwrap();
        assert_eq!((day.start.as_str(), day.end.as_str()), ("2026-11-24", "2026-11-25"));

        assert_eq!(p.delete(&ev.id, false).unwrap(), 1);
        assert!(p.get(&ev.id).is_err());
    }

    #[test]
    fn multi_day_all_day_events_cover_each_day() {
        let p = planner();
        p.create(EventInput { title: "Отпуск".into(), start: "2026-11-23".into(), end: Some("2026-11-26".into()), all_day: Some(true), ..Default::default() })
            .unwrap();
        for d in ["2026-11-23", "2026-11-24", "2026-11-25"] {
            let next = (NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap() + Duration::days(1)).format("%Y-%m-%d").to_string();
            assert_eq!(p.list(d, &next).unwrap().len(), 1, "{d}");
        }
        assert_eq!(p.list("2026-11-26", "2026-11-27").unwrap().len(), 0);
    }

    #[test]
    fn series_repeat_and_delete_following() {
        let p = planner();
        // 2026-11-23 is a Monday.
        let weekdays = p.create(EventInput { repeat: Some("weekdays".into()), ..input("Английский", "2026-11-23T08:00", Some("2026-11-23T08:20")) }).unwrap();
        assert_eq!(weekdays.len(), 30);
        assert!(weekdays.iter().all(|e| !e.start.starts_with("2026-11-28") && !e.start.starts_with("2026-11-29")));
        assert_eq!(weekdays[5].start, "2026-11-30T08:00");
        let weekly = p.create(EventInput { repeat: Some("weekly".into()), category: Some("sport".into()), ..input("Зал", "2026-11-25T18:00", Some("2026-11-25T19:00")) }).unwrap();
        assert_eq!(weekly.len(), 12);
        assert_eq!(weekly[1].start, "2026-12-02T18:00");
        assert_eq!(weekly[0].category, "sport");
        // Deleting the 4th and everything after leaves three.
        assert_eq!(p.delete(&weekly[3].id, true).unwrap(), 9);
        assert_eq!(p.list("2026-11-01", "2027-03-01").unwrap().iter().filter(|e| e.title == "Зал").count(), 3);
    }

    #[test]
    fn current_events_are_found() {
        let p = planner();
        p.create(input("Глубокая работа", "2026-11-23T09:00", Some("2026-11-23T11:00"))).unwrap();
        let at = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap();
        assert_eq!(p.current(at("2026-11-23T10:15")).unwrap().len(), 1);
        assert_eq!(p.current(at("2026-11-23T11:00")).unwrap().len(), 0);
        assert_eq!(p.current(at("2026-11-23T08:59")).unwrap().len(), 0);
    }

    #[test]
    fn palette_maps_to_google_color_ids() {
        assert_eq!(color_id("peacock"), Some(7));
        assert_eq!(color_by_id(11), Some("tomato"));
        assert_eq!(category_color("sport"), "basil");
        assert_eq!(category_label("study"), "Учёба");
        assert_eq!(normalize_category(Some("Work")), "work");
        assert_eq!(normalize_category(Some("что-то")), "personal");
    }
}

/// Today's date in local time.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}
