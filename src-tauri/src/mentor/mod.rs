//! The mentor: short videos get a daily budget (TikTok, Shorts, Reels, VK
//! clips; ordinary YouTube separately, learning videos free) and the minutes
//! are spent while they are on screen. When they run out — or at once during
//! focus, a work/study/project block or quiet hours — [`guard`] closes the
//! tab and opens «Перезагрузка»: a feed of short cards, games, a three-minute
//! workout, a breath, or ten minutes on a project. Growth earns XP, a streak
//! and extra video minutes (the Premack principle: the fun thing comes after
//! the useful one), so the quick reward stays — attached to the right habit.

pub mod guard;
pub mod seed;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::error::{AppError, AppResult};
use crate::store::Store;

pub const EVT_STATE: &str = "mentor:state";
pub const EVT_RESET: &str = "mentor:reset";
pub const EVT_WATCHING: &str = "mentor:watching";
pub const RESET_LABEL: &str = "reset";

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS mentor_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    at         TEXT NOT NULL,
    local_date TEXT NOT NULL,
    kind       TEXT NOT NULL,
    track      TEXT NOT NULL DEFAULT '',
    title      TEXT NOT NULL DEFAULT '',
    minutes    REAL NOT NULL DEFAULT 0,
    xp         INTEGER NOT NULL DEFAULT 0,
    earned_sec INTEGER NOT NULL DEFAULT 0,
    meta       TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS mentor_log_date ON mentor_log(local_date, kind);
CREATE TABLE IF NOT EXISTS mentor_usage (
    local_date TEXT NOT NULL,
    site       TEXT NOT NULL,
    seconds    INTEGER NOT NULL DEFAULT 0,
    closes     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(local_date, site)
);
CREATE TABLE IF NOT EXISTS mentor_cards (
    id         TEXT PRIMARY KEY,
    track      TEXT NOT NULL,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    source     TEXT NOT NULL DEFAULT 'seed',
    due        TEXT NOT NULL,
    interval   INTEGER NOT NULL DEFAULT 0,
    ease       REAL NOT NULL DEFAULT 2.5,
    reps       INTEGER NOT NULL DEFAULT 0,
    seen       INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS mentor_cards_due ON mentor_cards(track, seen, due);
"#;

/// Distracting sites: id, label, budget group.
pub const SITES: &[(&str, &str, &str)] = &[
    ("tiktok", "TikTok", "short"),
    ("shorts", "YouTube Shorts", "short"),
    ("reels", "Reels", "short"),
    ("vk", "VK Клипы", "short"),
    ("youtube", "YouTube", "youtube"),
];

pub fn site_label(site: &str) -> &'static str {
    SITES.iter().find(|(id, _, _)| *id == site).map(|(_, l, _)| *l).unwrap_or("Видео")
}

pub fn site_group(site: &str) -> &'static str {
    SITES.iter().find(|(id, _, _)| *id == site).map(|(_, _, g)| *g).unwrap_or("short")
}

/// A day counts towards the streak from this much XP (one 25-minute focus).
pub const GROWTH_XP: u32 = 50;
/// At most this many extra video minutes can be earned in a day.
const EARN_CAP_SEC: u32 = 20 * 60;

const LEVEL_NAMES: &[&str] = &["Новичок", "Ученик", "Практик", "Собранный", "Целеустремлённый", "Мастер фокуса", "Профи", "Эксперт", "Наставник", "Легенда"];

/// XP at which `level` (1-based) starts: 0, 100, 300, 600, 1000, …
pub fn level_floor(level: u32) -> u32 {
    50 * level * level.saturating_sub(1)
}

pub fn level_of(xp: u32) -> u32 {
    let mut l = 1;
    while level_floor(l + 1) <= xp {
        l += 1;
    }
    l
}

pub fn level_name(level: u32) -> &'static str {
    LEVEL_NAMES[(level.saturating_sub(1) as usize).min(LEVEL_NAMES.len() - 1)]
}

/// Consecutive growth days up to today (today counts once reached). One
/// missed day in any seven is forgiven — a slip is not a collapse.
pub fn streak(growth: &HashSet<NaiveDate>, today: NaiveDate) -> u32 {
    let mut day = if growth.contains(&today) { today } else { today - Duration::days(1) };
    let mut count = 0;
    let mut last_miss: Option<NaiveDate> = None;
    for _ in 0..3650 {
        if growth.contains(&day) {
            count += 1;
        } else {
            let forgiven = count > 0 && last_miss.is_none_or(|m| (m - day).num_days() >= 7);
            if !forgiven {
                break;
            }
            last_miss = Some(day);
            // Two misses in a row end it.
            if !growth.contains(&(day - Duration::days(1))) {
                break;
            }
        }
        day -= Duration::days(1);
    }
    count
}

/// «5 минут», «1 минута», «2 минуты».
pub fn minutes_ru(n: u32) -> String {
    let w = match (n % 10, n % 100) {
        (1, x) if x != 11 => "минута",
        (2..=4, x) if !(12..=14).contains(&x) => "минуты",
        _ => "минут",
    };
    format!("{n} {w}")
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub used_sec: u32,
    pub limit_sec: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Watching {
    pub site: String,
    pub label: String,
    pub used_sec: u32,
    pub limit_sec: u32,
    pub strict: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub date: String,
    pub xp: u32,
    pub short_min: u32,
    pub focus_min: u32,
    pub growth: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentorState {
    pub guard: bool,
    pub xp_total: u32,
    pub level: u32,
    pub level_name: String,
    pub level_from: u32,
    pub level_to: u32,
    pub today_xp: u32,
    pub goal_xp: u32,
    pub streak: u32,
    pub best_streak: u32,
    pub short: Budget,
    pub youtube: Budget,
    pub earned_sec: u32,
    pub strict: Option<String>,
    pub watching: Option<Watching>,
    pub closes_today: u32,
    pub focus_min_today: u32,
    pub week: Vec<DayStat>,
    pub if_then: String,
    pub tracks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Intervention {
    pub id: String,
    /// budget | strict | voluntary
    pub reason: String,
    pub site: String,
    pub label: String,
    pub used_sec: u32,
    pub strict: Option<String>,
    pub why: String,
    pub title: String,
    pub url: Option<String>,
    pub closed: bool,
    pub if_then: String,
    pub at: String,
    /// Open straight on this screen (feed, games, workout, breath, step).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedCard {
    pub id: String,
    pub track: String,
    pub kind: String,
    pub rare: bool,
    pub review: bool,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reward {
    pub xp: u32,
    pub earned_sec: u32,
    pub level: u32,
    pub level_name: String,
    pub level_up: bool,
    pub streak: u32,
    pub best: bool,
    pub record: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// event | task | repo
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub path: Option<String>,
    pub minutes: u32,
}

pub struct Mentor {
    app: Option<AppHandle>,
    store: Arc<Store>,
    /// YouTube titles the user said are learning, for today.
    allowed: Mutex<(String, HashSet<String>)>,
    pending: Mutex<Option<Intervention>>,
    pub(crate) watching: Mutex<Option<Watching>>,
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn day_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn clean(v: &Value, key: &str, max: usize) -> Option<String> {
    let s = v.get(key)?.as_str()?.trim();
    (!s.is_empty() && s.chars().count() <= max).then(|| s.to_string())
}

/// A card from the model or the seed, checked for shape.
pub fn valid_card(kind: &str, v: &Value) -> bool {
    match kind {
        "quiz" | "code" => {
            let opts = v.get("options").and_then(Value::as_array);
            let ok_opts = opts.is_some_and(|o| o.len() == 4 && o.iter().all(|x| x.as_str().is_some_and(|s| !s.trim().is_empty() && s.len() < 200)));
            let answer = v.get("answer").and_then(Value::as_u64).is_some_and(|a| a < 4);
            let head = if kind == "quiz" { clean(v, "q", 240).is_some() } else { clean(v, "code", 400).is_some() && clean(v, "title", 80).is_some() };
            ok_opts && answer && head && clean(v, "explain", 400).is_some()
        }
        "word" => clean(v, "word", 40).is_some() && clean(v, "tr", 60).is_some() && clean(v, "ex", 160).is_some(),
        "fact" => clean(v, "title", 80).is_some() && clean(v, "body", 420).is_some(),
        "challenge" => clean(v, "title", 80).is_some() && clean(v, "body", 300).is_some(),
        _ => false,
    }
}

impl Mentor {
    pub fn new(app: Option<AppHandle>, store: Arc<Store>) -> AppResult<Arc<Self>> {
        let m = Arc::new(Mentor { app, store, allowed: Mutex::new((String::new(), HashSet::new())), pending: Mutex::new(None), watching: Mutex::new(None) });
        m.seed()?;
        Ok(m)
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn app(&self) -> Option<&AppHandle> {
        self.app.as_ref()
    }

    /// New built-in cards arrive with updates; ones already seen keep their schedule.
    fn seed(&self) -> AppResult<()> {
        let cards: Vec<(String, String, Value)> = serde_json::from_str(seed::CARDS).map_err(|e| AppError::internal(format!("seed cards: {e}")))?;
        let now = Utc::now().to_rfc3339();
        let due = day_str(today());
        self.store.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for (i, (track, kind, payload)) in cards.iter().enumerate() {
                tx.execute(
                    "INSERT OR IGNORE INTO mentor_cards(id, track, kind, payload, source, due, created_at) VALUES(?1, ?2, ?3, ?4, 'seed', ?5, ?6)",
                    params![format!("seed-{i}"), track, kind, payload.to_string(), due, now],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    pub fn log(&self, kind: &str, track: &str, title: &str, minutes: f64, xp: u32, earned_sec: u32, meta: Value) -> AppResult<()> {
        let now = Local::now();
        self.store.with_conn(|conn| {
            conn.execute(
                "INSERT INTO mentor_log(at, local_date, kind, track, title, minutes, xp, earned_sec, meta) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![now.with_timezone(&Utc).to_rfc3339(), day_str(now.date_naive()), kind, track, title, minutes, xp, earned_sec, meta.to_string()],
            )?;
            Ok(())
        })
    }

    pub fn add_usage(&self, site: &str, seconds: u32, close: bool) -> AppResult<()> {
        self.store.with_conn(|conn| {
            conn.execute(
                "INSERT INTO mentor_usage(local_date, site, seconds, closes) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(local_date, site) DO UPDATE SET seconds = seconds + excluded.seconds, closes = closes + excluded.closes",
                params![day_str(today()), site, seconds, close as i64],
            )?;
            Ok(())
        })
    }

    /// Seconds per budget group spent on `date`.
    fn usage(&self, date: NaiveDate) -> AppResult<(u32, u32, u32)> {
        self.store.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT site, seconds, closes FROM mentor_usage WHERE local_date = ?1")?;
            let rows = stmt.query_map(params![day_str(date)], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
            let (mut short, mut yt, mut closes) = (0u32, 0u32, 0u32);
            for (site, sec, c) in rows.filter_map(Result::ok) {
                if site_group(&site) == "youtube" {
                    yt += sec.max(0) as u32;
                } else {
                    short += sec.max(0) as u32;
                }
                closes += c.max(0) as u32;
            }
            Ok((short, yt, closes))
        })
    }

    /// XP and focus minutes per local day since `from`.
    fn daily(&self, from: NaiveDate) -> AppResult<HashMap<NaiveDate, (u32, u32, u32)>> {
        let mut map: HashMap<NaiveDate, (u32, u32, u32)> = HashMap::new();
        let from_s = day_str(from);
        self.store.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT local_date, COALESCE(SUM(xp), 0), COALESCE(SUM(earned_sec), 0) FROM mentor_log WHERE local_date >= ?1 GROUP BY local_date")?;
            for (d, xp, earned) in stmt.query_map(params![from_s], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?.filter_map(Result::ok) {
                if let Ok(d) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                    let e = map.entry(d).or_default();
                    e.0 += xp.max(0) as u32;
                    e.2 += earned.max(0) as u32;
                }
            }
            // Focus pays 2 XP a minute; done tasks 10 each.
            let mut stmt = conn.prepare("SELECT local_date, COALESCE(SUM(minutes), 0) FROM focus_sessions WHERE local_date >= ?1 GROUP BY local_date")?;
            for (d, mins) in stmt.query_map(params![from_s], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.filter_map(Result::ok) {
                if let Ok(d) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                    let e = map.entry(d).or_default();
                    e.0 += 2 * mins.max(0) as u32;
                    e.1 += mins.max(0) as u32;
                }
            }
            let mut stmt = conn.prepare("SELECT completed_at FROM hub_tasks WHERE completed = 1 AND completed_at IS NOT NULL")?;
            for at in stmt.query_map([], |r| r.get::<_, String>(0))?.filter_map(Result::ok) {
                if let Ok(t) = DateTime::parse_from_rfc3339(&at) {
                    let d = t.with_timezone(&Local).date_naive();
                    if d >= from {
                        map.entry(d).or_default().0 += 10;
                    }
                }
            }
            Ok(())
        })?;
        Ok(map)
    }

    fn xp_total(&self) -> AppResult<u32> {
        self.store.with_conn(|conn| {
            let log: i64 = conn.query_row("SELECT COALESCE(SUM(xp), 0) FROM mentor_log", [], |r| r.get(0))?;
            let focus: i64 = conn.query_row("SELECT COALESCE(SUM(minutes), 0) FROM focus_sessions", [], |r| r.get(0))?;
            let tasks: i64 = conn.query_row("SELECT COUNT(*) FROM hub_tasks WHERE completed = 1", [], |r| r.get(0))?;
            Ok((log.max(0) + 2 * focus.max(0) + 10 * tasks.max(0)) as u32)
        })
    }

    /// Why short videos are closed at once right now, if they are.
    pub fn strict_reason(&self) -> Option<String> {
        let s = self.store.settings().mentor;
        let now = Local::now();
        if s.strict_focus {
            if let Some(app) = &self.app {
                if let Some(hub) = app.try_state::<Arc<crate::hub::Hub>>() {
                    if let Ok(f) = hub.focus_get() {
                        if f.active && !f.paused {
                            return Some(format!("Идёт фокус «{}»", f.title));
                        }
                    }
                }
                if let Some(planner) = app.try_state::<Arc<crate::planner::Planner>>() {
                    if let Ok(evs) = planner.current(now.naive_local()) {
                        if let Some(e) = evs.iter().find(|e| !e.done && matches!(e.category.as_str(), "work" | "study" | "project")) {
                            return Some(format!("Сейчас в календаре «{}»", e.title));
                        }
                    }
                }
            }
        }
        let parse = |t: &str| NaiveTime::parse_from_str(t.trim(), "%H:%M").ok();
        if let (Some(a), Some(b)) = (parse(&s.night_from), parse(&s.night_to)) {
            let t = now.time();
            let inside = if a <= b { t >= a && t < b } else { t >= a || t < b };
            if inside {
                return Some("Тихие часы".into());
            }
        }
        None
    }

    /// Budgets for today: (short, youtube, earned seconds).
    pub fn budgets(&self) -> AppResult<(Budget, Budget, u32)> {
        let s = self.store.settings().mentor;
        let (short_used, yt_used, _) = self.usage(today())?;
        let d = self.daily(today())?.get(&today()).copied().unwrap_or_default();
        // 25 minutes of focus earn 5 video minutes; logged growth adds its own.
        let earned = if s.earn { (d.2 + d.1 * 12).min(EARN_CAP_SEC) } else { 0 };
        let strict = self.strict_reason().is_some();
        let limit = |base: u32| if strict { 0 } else { base * 60 + earned };
        Ok((Budget { used_sec: short_used, limit_sec: limit(s.short_minutes) }, Budget { used_sec: yt_used, limit_sec: limit(s.youtube_minutes) }, earned))
    }

    pub fn state(&self) -> AppResult<MentorState> {
        let s = self.store.settings().mentor;
        let t = today();
        let days = self.daily(t - Duration::days(400))?;
        let growth: HashSet<NaiveDate> = days.iter().filter(|(_, v)| v.0 >= GROWTH_XP).map(|(d, _)| *d).collect();
        let streak_now = streak(&growth, t);
        let best = {
            let mut best = streak_now;
            let mut sorted: Vec<&NaiveDate> = growth.iter().collect();
            sorted.sort();
            let mut run = 0;
            let mut prev: Option<NaiveDate> = None;
            for d in sorted {
                run = if prev.is_some_and(|p| (*d - p).num_days() <= 2) { run + 1 } else { 1 };
                best = best.max(run);
                prev = Some(*d);
            }
            best
        };
        let xp_total = self.xp_total()?;
        let level = level_of(xp_total);
        let (short, youtube, earned_sec) = self.budgets()?;
        let (_, _, closes) = self.usage(t)?;
        let mut week = Vec::new();
        for i in (0..7).rev() {
            let d = t - Duration::days(i);
            let v = days.get(&d).copied().unwrap_or_default();
            let (short_sec, _, _) = self.usage(d)?;
            week.push(DayStat { date: day_str(d), xp: v.0, short_min: short_sec / 60, focus_min: v.1, growth: v.0 >= GROWTH_XP });
        }
        let today_v = days.get(&t).copied().unwrap_or_default();
        Ok(MentorState {
            guard: s.guard,
            xp_total,
            level,
            level_name: level_name(level).into(),
            level_from: level_floor(level),
            level_to: level_floor(level + 1),
            today_xp: today_v.0,
            goal_xp: GROWTH_XP,
            streak: streak_now,
            best_streak: best,
            short,
            youtube,
            earned_sec,
            strict: self.strict_reason(),
            watching: self.watching.lock().clone(),
            closes_today: closes,
            focus_min_today: today_v.1,
            week,
            if_then: s.if_then,
            tracks: s.tracks,
        })
    }

    pub fn emit_state(&self) {
        if let (Some(app), Ok(st)) = (&self.app, self.state()) {
            let _ = app.emit(EVT_STATE, st);
        }
    }

    // ---------- learning allowance ----------

    pub fn allow_title(&self, title: &str) {
        let mut a = self.allowed.lock();
        let d = day_str(today());
        if a.0 != d {
            *a = (d, HashSet::new());
        }
        a.1.insert(title.trim().to_lowercase());
    }

    pub fn is_allowed(&self, title: &str) -> bool {
        let a = self.allowed.lock();
        a.0 == day_str(today()) && a.1.contains(&title.trim().to_lowercase())
    }

    // ---------- the reset window ----------

    pub fn pending(&self) -> Option<Intervention> {
        self.pending.lock().clone()
    }

    pub fn open_reset(&self, iv: Intervention) -> AppResult<()> {
        *self.pending.lock() = Some(iv.clone());
        let Some(app) = &self.app else { return Ok(()) };
        if let Some(w) = app.get_webview_window(RESET_LABEL) {
            let _ = w.unminimize();
            w.show()?;
            w.set_focus()?;
        } else {
            let w = WebviewWindowBuilder::new(app, RESET_LABEL, WebviewUrl::App("index.html".into()))
                .title("Джарвис — перезагрузка")
                .inner_size(920.0, 640.0)
                .resizable(false)
                .maximizable(false)
                .center()
                .theme(Some(tauri::Theme::Light))
                .background_color(tauri::webview::Color(243, 244, 246, 255))
                .decorations(false)
                .shadow(true)
                .always_on_top(true)
                .build()?;
            // On top only long enough to come in front of the browser.
            let w2 = w.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                let _ = w2.set_always_on_top(false);
            });
        }
        let _ = app.emit_to(RESET_LABEL, EVT_RESET, &iv);
        Ok(())
    }

    pub fn voluntary(&self, screen: Option<String>) -> AppResult<()> {
        let s = self.store.settings().mentor;
        let screen = screen.filter(|x| matches!(x.as_str(), "feed" | "games" | "workout" | "breath" | "step"));
        self.open_reset(Intervention {
            id: uuid::Uuid::new_v4().to_string(),
            reason: "voluntary".into(),
            why: String::new(),
            if_then: s.if_then,
            at: Utc::now().to_rfc3339(),
            screen,
            ..Default::default()
        })
    }

    // ---------- cards ----------

    fn card_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, String, String, i64)> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    }

    /// A set of `n` cards: due reviews first, then new ones across the
    /// user's tracks, one body challenge in the middle, sometimes a rare one.
    pub fn feed(&self, n: usize) -> AppResult<Vec<FeedCard>> {
        let tracks: Vec<String> = self
            .store
            .settings()
            .mentor
            .tracks
            .into_iter()
            .filter(|t| matches!(t.as_str(), "code" | "english" | "body"))
            .collect();
        let tracks = if tracks.is_empty() { vec!["code".into(), "english".into(), "body".into()] } else { tracks };
        let today_s = day_str(today());
        let list = tracks.iter().map(|t| format!("'{t}'")).collect::<Vec<_>>().join(",");
        let (due, fresh, challenge, words) = self.store.with_conn(|conn| {
            let q = |sql: &str| -> AppResult<Vec<(String, String, String, String, i64)>> {
                let mut stmt = conn.prepare(sql)?;
                let rows = stmt.query_map(params![today_s], Mentor::card_row)?;
                Ok(rows.filter_map(Result::ok).collect())
            };
            let due = q(&format!("SELECT id, track, kind, payload, seen FROM mentor_cards WHERE seen > 0 AND due <= ?1 AND kind != 'challenge' AND track IN ({list}) ORDER BY due LIMIT 2"))?;
            let fresh = q(&format!("SELECT id, track, kind, payload, seen FROM mentor_cards WHERE (seen = 0 OR ?1 = '') AND kind != 'challenge' AND track IN ({list}) ORDER BY RANDOM() LIMIT 24"))?;
            let challenge = if tracks.iter().any(|t| t == "body") {
                q("SELECT id, track, kind, payload, seen FROM mentor_cards WHERE track = 'body' AND kind = 'challenge' AND (?1 = ?1) ORDER BY seen, RANDOM() LIMIT 1")?
            } else {
                Vec::new()
            };
            let words = q("SELECT id, track, kind, payload, seen FROM mentor_cards WHERE kind = 'word' AND (?1 = ?1) ORDER BY RANDOM() LIMIT 40")?;
            Ok((due, fresh, challenge, words))
        })?;
        // Everything seen already: cycle through the oldest instead.
        let fresh = if fresh.is_empty() {
            self.store.with_conn(|conn| {
                let mut stmt = conn.prepare(&format!("SELECT id, track, kind, payload, seen FROM mentor_cards WHERE kind != 'challenge' AND track IN ({list}) ORDER BY due, RANDOM() LIMIT 24"))?;
                let rows = stmt.query_map([], Mentor::card_row)?;
                Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
            })?
        } else {
            fresh
        };
        let translations: Vec<String> = words
            .iter()
            .filter_map(|(_, _, _, p, _)| serde_json::from_str::<Value>(p).ok()?.get("tr")?.as_str().map(str::to_string))
            .collect();

        // Interleave tracks so the set feels varied.
        let mut by_track: HashMap<String, Vec<(String, String, String, String, i64)>> = HashMap::new();
        for c in fresh {
            by_track.entry(c.1.clone()).or_default().push(c);
        }
        let mut picked: Vec<((String, String, String, String, i64), bool)> = due.into_iter().map(|c| (c, true)).collect();
        let mut seen_ids: HashSet<String> = picked.iter().map(|(c, _)| c.0.clone()).collect();
        let order: Vec<String> = tracks.iter().filter(|t| *t != "body").cloned().chain(tracks.iter().filter(|t| *t == "body").cloned()).collect();
        let budget = n.saturating_sub(challenge.len());
        'fill: while picked.len() < budget {
            let mut progressed = false;
            for t in &order {
                if picked.len() >= budget {
                    break 'fill;
                }
                if let Some(list) = by_track.get_mut(t) {
                    while let Some(c) = list.pop() {
                        if seen_ids.insert(c.0.clone()) {
                            picked.push((c, false));
                            progressed = true;
                            break;
                        }
                    }
                }
            }
            if !progressed {
                break;
            }
        }
        if let Some(c) = challenge.into_iter().next() {
            let mid = (picked.len() / 2).min(picked.len());
            picked.insert(mid, (c, false));
        }
        let seed = Utc::now().timestamp_subsec_nanos() as usize;
        let rare_at = if picked.len() >= 4 && seed % 3 == 0 { Some(1 + seed % (picked.len() - 1)) } else { None };
        Ok(picked
            .into_iter()
            .enumerate()
            .filter_map(|(i, ((id, track, kind, payload, _), review))| {
                let mut data: Value = serde_json::from_str(&payload).ok()?;
                if kind == "word" {
                    let tr = data.get("tr")?.as_str()?.to_string();
                    let mut options: Vec<String> = translations.iter().filter(|t| **t != tr).take(3 + (i % 5)).skip(i % 5).cloned().collect();
                    options.truncate(3);
                    if options.len() < 3 {
                        for extra in ["возможно", "срочно", "актуально", "аккуратно"] {
                            if options.len() < 3 && extra != tr {
                                options.push(extra.into());
                            }
                        }
                    }
                    let answer = (seed + i) % 4;
                    options.insert(answer, tr);
                    data["options"] = json!(options);
                    data["answer"] = json!(answer);
                }
                Some(FeedCard { id, track, kind, rare: rare_at == Some(i), review, data })
            })
            .collect())
    }

    /// Spaced repetition (SM-2): a miss comes back tomorrow, a hit later
    /// each time. Logs the XP the card earned.
    pub fn answer(&self, id: &str, correct: bool, quick: bool, xp: u32) -> AppResult<()> {
        let row: Option<(String, i64, f64, i64)> = self.store.with_conn(|conn| {
            Ok(conn
                .query_row("SELECT track, interval, ease, reps FROM mentor_cards WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .optional()?)
        })?;
        let Some((track, interval, ease, reps)) = row else { return Err(AppError::not_found("Карточка не найдена")) };
        let q: f64 = if correct { if quick { 5.0 } else { 4.0 } } else { 2.0 };
        let (reps, interval) = if q < 3.0 {
            (0, 1)
        } else {
            let reps = reps + 1;
            (reps, if reps == 1 { 1 } else if reps == 2 { 3 } else { ((interval.max(1) as f64) * ease).round() as i64 })
        };
        let ease = (ease + 0.1 - (5.0 - q) * (0.08 + (5.0 - q) * 0.02)).max(1.3);
        let due = day_str(today() + Duration::days(interval));
        self.store.with_conn(|conn| {
            conn.execute(
                "UPDATE mentor_cards SET interval = ?2, ease = ?3, reps = ?4, due = ?5, seen = seen + 1 WHERE id = ?1",
                params![id, interval, ease, reps, due],
            )?;
            Ok(())
        })?;
        self.log("card", &track, if correct { "верно" } else { "ошибка" }, 0.0, xp.min(60), 0, json!({ "card": id }))?;
        self.emit_state();
        Ok(())
    }

    /// A finished activity: feed set, game, workout, breath, lesson, review.
    pub fn complete(&self, kind: &str, track: &str, title: &str, minutes: f64, score: u32, xp: u32, after_close: bool) -> AppResult<Reward> {
        let before = level_of(self.xp_total()?);
        let earned = match kind {
            "feed" | "workout" => 180,
            "game" => 120,
            "breath" => 60,
            "lesson" => 300,
            _ => 0,
        };
        let mut xp = xp.min(400);
        if after_close {
            // Chose growth right after the pull: the moment worth rewarding most.
            xp += 15;
        }
        let mut best = false;
        let mut record = score;
        if kind == "game" {
            let key = format!("mentor_best_{}", title.to_lowercase().replace(' ', "_"));
            let prev: u32 = self.store.kv_get(&key).and_then(|v| v.parse().ok()).unwrap_or(0);
            best = score > prev;
            record = prev.max(score);
            if best {
                self.store.kv_set(&key, &score.to_string())?;
            }
        }
        self.log(kind, track, title, minutes, xp, earned, json!({ "score": score, "afterClose": after_close }))?;
        let st = self.state()?;
        self.emit_state();
        Ok(Reward { xp, earned_sec: earned, level: st.level, level_name: st.level_name, level_up: st.level > before, streak: st.streak, best, record })
    }

    /// Word pairs for the speed game.
    pub fn words(&self, n: usize) -> AppResult<Vec<(String, String)>> {
        self.store.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT payload FROM mentor_cards WHERE kind = 'word' ORDER BY RANDOM() LIMIT ?1")?;
            let rows = stmt.query_map(params![n as i64], |r| r.get::<_, String>(0))?;
            Ok(rows
                .filter_map(Result::ok)
                .filter_map(|p| {
                    let v: Value = serde_json::from_str(&p).ok()?;
                    Some((v.get("word")?.as_str()?.to_string(), v.get("tr")?.as_str()?.to_string()))
                })
                .collect())
        })
    }

    pub fn best(&self, game: &str) -> u32 {
        self.store.kv_get(&format!("mentor_best_{}", game.to_lowercase().replace(' ', "_"))).and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    pub fn workout(&self) -> Value {
        let all: Vec<Value> = serde_json::from_str(seed::WORKOUTS).unwrap_or_default();
        let i = (today().num_days_from_ce().max(0) as usize) % all.len().max(1);
        all.get(i).cloned().unwrap_or(json!([]))
    }

    /// The evening review: what went well, what was learned (retrieval),
    /// tomorrow's one most important thing (becomes a task).
    pub fn review(&self, went_well: &str, learned: &str, tomorrow: &str) -> AppResult<Reward> {
        if let (Some(app), false) = (&self.app, tomorrow.trim().is_empty()) {
            if let Some(hub) = app.try_state::<Arc<crate::hub::Hub>>() {
                let _ = hub.task_add_with_notes(tomorrow.trim().to_string(), "Главное на день — из вечернего итога".into(), Some("tomorrow".into()), None, Some("amber".into()));
            }
        }
        self.log("review_note", "", "Итог дня", 0.0, 0, 0, json!({ "wentWell": went_well, "learned": learned, "tomorrow": tomorrow }))?;
        self.complete("review", "", "Итог дня", 0.0, 0, 30, false)
    }

    // ---------- project steps ----------

    pub fn steps(&self) -> AppResult<Vec<Step>> {
        let mut out = Vec::new();
        let now = Local::now().naive_local();
        if let Some(app) = &self.app {
            if let Some(planner) = app.try_state::<Arc<crate::planner::Planner>>() {
                for e in planner.day(now.date())? {
                    let end = crate::planner::parse_when(&e.end);
                    let ahead = matches!(end, Some(crate::planner::When::At(t)) if t > now);
                    if !e.done && !e.all_day && ahead && matches!(e.category.as_str(), "work" | "project" | "study") {
                        out.push(Step { kind: "event".into(), subtitle: format!("В календаре на {}", e.start.get(11..).unwrap_or("")), title: e.title, path: None, minutes: 15 });
                    }
                }
            }
            if let Some(hub) = app.try_state::<Arc<crate::hub::Hub>>() {
                let d = day_str(now.date());
                for t in hub.tasks_list()?.into_iter().filter(|t| !t.completed && t.scheduled_date.as_deref().is_none_or(|x| x <= d.as_str())).take(3) {
                    out.push(Step { kind: "task".into(), subtitle: "Задача".into(), title: t.title, path: None, minutes: 10 });
                }
            }
        }
        let roots = self.store.settings().agent.project_roots;
        let mut repos: Vec<(i64, Step)> = Vec::new();
        for root in roots.iter().map(|r| PathBuf::from(r.trim())).filter(|p| p.is_dir()) {
            let mut dirs = vec![root.clone()];
            if !root.join(".git").exists() {
                if let Ok(rd) = std::fs::read_dir(&root) {
                    dirs = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.join(".git").exists()).take(12).collect();
                }
            }
            for dir in dirs.into_iter().filter(|d| d.join(".git").exists()) {
                if let Some((ts, subject)) = last_commit(&dir) {
                    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    repos.push((ts, Step { kind: "repo".into(), title: name, subtitle: format!("Последний коммит: «{subject}»"), path: Some(dir.display().to_string()), minutes: 10 }));
                }
            }
        }
        repos.sort_by(|a, b| b.0.cmp(&a.0));
        out.extend(repos.into_iter().take(3).map(|(_, s)| s));
        Ok(out)
    }
}

/// (commit time, subject) of HEAD.
fn last_commit(dir: &Path) -> Option<(i64, String)> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dir).args(["log", "-1", "--format=%ct%x1f%s"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let (ts, subject) = text.split_once('\u{1f}')?;
    let subject: String = subject.chars().take(70).collect();
    Some((ts.parse().ok()?, subject))
}

/// Ask the model for fresh cards when a track runs low (at most every six
/// hours per track).
pub async fn refill(app: AppHandle, mentor: Arc<Mentor>) {
    let state = app.state::<crate::state::AppState>();
    let settings = state.store.settings();
    let Ok(client) = crate::ai::client_from_settings(&settings, &state.store) else { return };
    for track in settings.mentor.tracks.iter().filter(|t| matches!(t.as_str(), "code" | "english")) {
        let key = format!("mentor_refill_{track}");
        let recent = mentor.store.kv_get(&key).and_then(|v| DateTime::parse_from_rfc3339(&v).ok()).is_some_and(|t| Utc::now() - t.with_timezone(&Utc) < Duration::hours(6));
        let unseen: i64 = mentor
            .store
            .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM mentor_cards WHERE track = ?1 AND seen = 0", params![track], |r| r.get(0))?))
            .unwrap_or(0);
        if recent || unseen >= 8 {
            continue;
        }
        let _ = mentor.store.kv_set(&key, &Utc::now().to_rfc3339());
        let known: Vec<String> = mentor
            .store
            .with_conn(|c| {
                let mut stmt = c.prepare("SELECT payload FROM mentor_cards WHERE track = ?1 ORDER BY created_at DESC LIMIT 60")?;
                let rows = stmt.query_map(params![track], |r| r.get::<_, String>(0))?;
                Ok(rows
                    .filter_map(Result::ok)
                    .filter_map(|p| serde_json::from_str::<Value>(&p).ok())
                    .filter_map(|v| v.get("word").or_else(|| v.get("q")).or_else(|| v.get("title")).and_then(Value::as_str).map(str::to_string))
                    .collect())
            })
            .unwrap_or_default();
        let topic = if track == "code" {
            "программирование: Rust, TypeScript/JavaScript, Python, Git, SQL, алгоритмы, сети, архитектура. Типы: quiz {q, options[4], answer, explain}, code {title, code (до 6 строк), options[4], answer, explain}, fact {title, body до 280 символов}"
        } else {
            "английский для разработчика, уровень B1–B2: полезные слова, фразовые глаголы, ложные друзья переводчика. Типы: word {word, ipa, tr (перевод), ex (пример на английском), exRu (перевод примера)}, quiz {q, options[4], answer, explain}"
        };
        let prompt = format!(
            "Сделай 10 новых карточек для короткой обучающей ленты. Тема: {topic}. Каждая начинается с крючка, \
             коротко, без воды, по-русски (английские слова и код — как есть), факты точные. Не повторяй: {known}.\n\
             Ответ — только JSON: {{\"cards\":[{{\"kind\":\"quiz\", ...поля...}}]}}",
            known = known.join("; ")
        );
        let msgs = [
            crate::ai::types::ChatMessage::text(crate::ai::types::Role::System, "Ты автор коротких обучающих карточек. Отвечаешь строго JSON."),
            crate::ai::types::ChatMessage::text(crate::ai::types::Role::User, prompt),
        ];
        let cancel = tokio_util::sync::CancellationToken::new();
        let Ok(Ok(done)) = tokio::time::timeout(std::time::Duration::from_secs(60), client.complete(&msgs, &[], &cancel, Some(2500), |_| {})).await else { continue };
        let text = done.message.joined_text();
        let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text[a..=b]) else { continue };
        let now = Utc::now().to_rfc3339();
        let due = day_str(today());
        let mut added = 0;
        for c in v.get("cards").and_then(Value::as_array).cloned().unwrap_or_default() {
            let kind = c.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
            let allowed = if track == "code" { matches!(kind.as_str(), "quiz" | "code" | "fact") } else { matches!(kind.as_str(), "word" | "quiz") };
            if !allowed || !valid_card(&kind, &c) {
                continue;
            }
            let mut payload = c.clone();
            if let Some(o) = payload.as_object_mut() {
                o.remove("kind");
            }
            let id = format!("ai-{}", uuid::Uuid::new_v4().simple());
            let ok = mentor.store.with_conn(|conn| {
                Ok(conn.execute(
                    "INSERT INTO mentor_cards(id, track, kind, payload, source, due, created_at) VALUES(?1, ?2, ?3, ?4, 'ai', ?5, ?6)",
                    params![id, track, kind, payload.to_string(), due, now],
                )?)
            });
            if ok.is_ok() {
                added += 1;
            }
        }
        tracing::info!(track = %track, added, "mentor: fresh cards");
    }
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn levels_grow_slower_over_time() {
        assert_eq!(level_of(0), 1);
        assert_eq!(level_of(99), 1);
        assert_eq!(level_of(100), 2);
        assert_eq!(level_of(299), 2);
        assert_eq!(level_of(300), 3);
        assert_eq!(level_of(1000), 5);
        assert_eq!(level_name(1), "Новичок");
        assert_eq!(level_name(99), "Легенда");
    }

    #[test]
    fn streak_forgives_one_slip_a_week() {
        let t = d("2026-11-23");
        let days = |list: &[&str]| list.iter().map(|s| d(s)).collect::<HashSet<_>>();
        assert_eq!(streak(&days(&["2026-11-21", "2026-11-22", "2026-11-23"]), t), 3);
        // Today not reached yet: yesterday's run still stands.
        assert_eq!(streak(&days(&["2026-11-21", "2026-11-22"]), t), 2);
        // One missed day (20th) is forgiven.
        assert_eq!(streak(&days(&["2026-11-18", "2026-11-19", "2026-11-21", "2026-11-22", "2026-11-23"]), t), 5);
        // Two misses in a row end the run.
        assert_eq!(streak(&days(&["2026-11-17", "2026-11-18", "2026-11-21", "2026-11-22", "2026-11-23"]), t), 3);
        // Two separate misses within a week: the second one ends it.
        assert_eq!(streak(&days(&["2026-11-16", "2026-11-18", "2026-11-20", "2026-11-21", "2026-11-22", "2026-11-23"]), t), 5);
        assert_eq!(streak(&HashSet::new(), t), 0);
    }

    #[test]
    fn russian_minutes() {
        assert_eq!(minutes_ru(1), "1 минута");
        assert_eq!(minutes_ru(3), "3 минуты");
        assert_eq!(minutes_ru(11), "11 минут");
        assert_eq!(minutes_ru(12), "12 минут");
        assert_eq!(minutes_ru(21), "21 минута");
        assert_eq!(minutes_ru(25), "25 минут");
    }

    #[test]
    fn seed_cards_are_well_formed() {
        let cards: Vec<(String, String, Value)> = serde_json::from_str(seed::CARDS).unwrap();
        assert!(cards.len() >= 80);
        for (track, kind, v) in &cards {
            assert!(matches!(track.as_str(), "code" | "english" | "body"), "{track}");
            assert!(valid_card(kind, v), "bad card: {kind} {v}");
        }
        let w: Vec<Value> = serde_json::from_str(seed::WORKOUTS).unwrap();
        assert!(w.iter().all(|x| x.as_array().is_some_and(|a| a.len() == 3)));
    }

    #[test]
    fn feed_mixes_tracks_and_schedules_reviews() {
        let m = Mentor::new(None, Arc::new(Store::in_memory().unwrap())).unwrap();
        let set = m.feed(7).unwrap();
        assert_eq!(set.len(), 7);
        assert!(set.iter().any(|c| c.track == "code") && set.iter().any(|c| c.track == "english"));
        assert_eq!(set.iter().filter(|c| c.kind == "challenge").count(), 1);
        for c in set.iter().filter(|c| c.kind == "word") {
            let opts = c.data["options"].as_array().unwrap();
            assert_eq!(opts.len(), 4);
            assert_eq!(opts[c.data["answer"].as_u64().unwrap() as usize], c.data["tr"]);
        }
        // A miss comes back tomorrow; a hit not before.
        let card = set.iter().find(|c| c.kind != "challenge").unwrap();
        m.answer(&card.id, false, false, 3).unwrap();
        let due: String = m.store.with_conn(|c| Ok(c.query_row("SELECT due FROM mentor_cards WHERE id = ?1", params![card.id], |r| r.get(0))?)).unwrap();
        assert_eq!(due, day_str(today() + Duration::days(1)));
        let st = m.state().unwrap();
        assert_eq!(st.today_xp, 3);
    }

    #[test]
    fn reset_opens_on_a_known_screen_only() {
        let m = Mentor::new(None, Arc::new(Store::in_memory().unwrap())).unwrap();
        m.voluntary(Some("workout".into())).unwrap();
        let iv = m.pending().unwrap();
        assert_eq!((iv.reason.as_str(), iv.screen.as_deref()), ("voluntary", Some("workout")));
        m.voluntary(Some("settings".into())).unwrap();
        assert_eq!(m.pending().unwrap().screen, None);
        m.voluntary(None).unwrap();
        assert_eq!(m.pending().unwrap().screen, None);
    }

    #[test]
    fn growth_earns_minutes_and_xp() {
        let m = Mentor::new(None, Arc::new(Store::in_memory().unwrap())).unwrap();
        let before = m.budgets().unwrap();
        assert_eq!(before.0.limit_sec, 10 * 60);
        let r = m.complete("workout", "body", "Разминка", 3.0, 0, 40, true).unwrap();
        assert_eq!(r.xp, 55);
        assert_eq!(r.earned_sec, 180);
        let after = m.budgets().unwrap();
        assert_eq!(after.0.limit_sec, 10 * 60 + 180);
        assert_eq!(after.1.limit_sec, 30 * 60 + 180);
        m.add_usage("tiktok", 125, false).unwrap();
        m.add_usage("youtube", 60, true).unwrap();
        let (short, yt, _) = m.budgets().unwrap();
        assert_eq!(short.used_sec, 125);
        assert_eq!(yt.used_sec, 60);
        let g = m.complete("game", "code", "Спринт счёта", 1.0, 17, 17, false).unwrap();
        assert!(g.best);
        let g2 = m.complete("game", "code", "Спринт счёта", 1.0, 12, 12, false).unwrap();
        assert!(!g2.best);
        assert_eq!(g2.record, 17);
        assert_eq!(m.best("Спринт счёта"), 17);
    }
}
