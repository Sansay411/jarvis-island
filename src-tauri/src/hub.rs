//! Notch Hub subsystem: persistent Tasks & Agenda, Focus Session Engine with
//! 1-second active ticker, Holding Shelf (Tray), Real 52-Week Activity & Streak
//! Heatmap (SQLite + local Git commits), and Autonomous Routines / Playbooks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, NaiveTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::Store;

pub const EVT_TASKS: &str = "hub:tasks";
pub const EVT_FOCUS: &str = "hub:focus";
pub const EVT_SHELF: &str = "hub:shelf";
pub const EVT_ROUTINES: &str = "hub:routines";
pub const EVT_TTS_REQUEST: &str = "tts:request";

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS hub_tasks (
    id             TEXT PRIMARY KEY,
    title          TEXT NOT NULL,
    notes          TEXT NOT NULL DEFAULT '',
    scheduled_date TEXT,
    scheduled_time TEXT,
    color          TEXT NOT NULL DEFAULT 'blue',
    completed      INTEGER NOT NULL DEFAULT 0,
    completed_at   TEXT,
    created_at     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS hub_tasks_order ON hub_tasks(completed, scheduled_date, scheduled_time, created_at);

CREATE TABLE IF NOT EXISTS focus_sessions (
    id           TEXT PRIMARY KEY,
    task_id      TEXT,
    title        TEXT NOT NULL,
    planned_sec  INTEGER NOT NULL,
    elapsed_sec  INTEGER NOT NULL,
    minutes      INTEGER NOT NULL,
    started_at   TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    local_date   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS focus_sessions_date ON focus_sessions(local_date);

CREATE TABLE IF NOT EXISTS shelf_items (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    label      TEXT NOT NULL,
    value      TEXT NOT NULL,
    subtitle   TEXT NOT NULL DEFAULT '',
    size_bytes INTEGER,
    pinned     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS shelf_items_order ON shelf_items(pinned DESC, created_at DESC);

CREATE TABLE IF NOT EXISTS routines (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    subtitle   TEXT NOT NULL DEFAULT '',
    icon       TEXT NOT NULL DEFAULT 'zap',
    prompt     TEXT NOT NULL,
    autonomy   INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
"#;

const VALID_COLORS: &[&str] = &["blue", "amber", "coral", "emerald", "violet"];
const GIT_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub scheduled_date: Option<String>,
    pub scheduled_time: Option<String>,
    pub color: String,
    pub completed: bool,
    pub completed_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FocusState {
    pub active: bool,
    pub paused: bool,
    pub task_id: Option<String>,
    pub title: String,
    pub total_sec: u32,
    pub remaining_sec: u32,
    pub started_at: Option<String>,
    pub ends_at: Option<String>,
    pub completed_today: u32,
    pub minutes_today: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShelfItem {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub value: String,
    pub subtitle: String,
    pub size_bytes: Option<u64>,
    pub exists: bool,
    pub pinned: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDay {
    pub date: String,
    pub count: u32,
    pub level: u8,
    pub focus_minutes: u32,
    pub tasks_done: u32,
    pub agent_actions: u32,
    pub git_commits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityHeatmap {
    pub user_name: String,
    pub total_year: u32,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub days: Vec<ActivityDay>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoutineItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    pub prompt: String,
    pub autonomy: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsRequestPayload {
    pub text: String,
    pub voice: Option<String>,
    pub rate: Option<f32>,
    pub interrupt: bool,
}

/// Central TTS dispatch hook used by both `tools::executive::speak_aloud` and
/// Focus session completion. Speaks directly via `VoiceManager::speak` when
/// managed in Tauri state (emitting `"tts:state"` and `"voice:level"` without
/// opening the microphone or double-firing `"tts:request"`).
pub fn request_tts(app: &AppHandle, text: &str, voice: Option<String>, rate: Option<f32>, interrupt: bool) {
    let clean = text.trim();
    if clean.is_empty() {
        return;
    }
    if let Some(vm) = app.try_state::<Arc<crate::voice::VoiceManager>>() {
        let _ = vm.speak(clean.to_string(), voice, rate, interrupt);
        return;
    }
    let payload = TtsRequestPayload {
        text: clean.to_string(),
        voice,
        rate,
        interrupt,
    };
    let _ = app.emit(EVT_TTS_REQUEST, &payload);
}

#[derive(Debug, Clone, Default)]
pub struct RawDayCounts {
    pub focus_minutes: u32,
    pub focus_sessions: u32,
    pub tasks_done: u32,
    pub agent_actions: u32,
    pub git_commits: u32,
}

#[derive(Debug, Clone)]
struct ActiveFocus {
    task_id: Option<String>,
    title: String,
    total_sec: u32,
    remaining_sec: u32,
    paused: bool,
    started_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct FocusRuntime {
    gen: u64,
    current: Option<ActiveFocus>,
}

pub struct Hub {
    app: Option<AppHandle>,
    store: Arc<Store>,
    focus: Mutex<FocusRuntime>,
    git_cache: Mutex<Option<(Instant, HashMap<NaiveDate, u32>)>>,
    scan_system_repos: bool,
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn today_local_str() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn normalize_color(color: Option<&str>) -> String {
    let c = color.unwrap_or("blue").trim().to_lowercase();
    if VALID_COLORS.contains(&c.as_str()) {
        c
    } else {
        "blue".to_string()
    }
}

fn normalize_date(date: Option<String>) -> AppResult<Option<String>> {
    let Some(raw) = date else { return Ok(None) };
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let lower = s.to_lowercase();
    if lower == "today" || lower == "сегодня" {
        return Ok(Some(today_local_str()));
    }
    if lower == "tomorrow" || lower == "завтра" {
        let d = Local::now().date_naive() + chrono::Duration::days(1);
        return Ok(Some(d.format("%Y-%m-%d").to_string()));
    }
    let parsed = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| AppError::invalid(format!("Некорректная дата «{s}» (ожидается YYYY-MM-DD)")))?;
    Ok(Some(parsed.format("%Y-%m-%d").to_string()))
}

fn normalize_time(time: Option<String>) -> AppResult<Option<String>> {
    let Some(raw) = time else { return Ok(None) };
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let parsed = NaiveTime::parse_from_str(s, "%H:%M")
        .map_err(|_| AppError::invalid(format!("Некорректное время «{s}» (ожидается HH:MM)")))?;
    Ok(Some(parsed.format("%H:%M").to_string()))
}

fn parse_local_date(ts: &str) -> Option<NaiveDate> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&Local).date_naive());
    }
    if ts.len() >= 10 {
        if let Ok(d) = NaiveDate::parse_from_str(&ts[..10], "%Y-%m-%d") {
            return Some(d);
        }
    }
    None
}

impl Hub {
    pub fn new(app: AppHandle) -> AppResult<Arc<Self>> {
        let store = app.state::<AppState>().store.clone();
        let hub = Arc::new(Hub {
            app: Some(app),
            store,
            focus: Mutex::new(FocusRuntime::default()),
            git_cache: Mutex::new(None),
            scan_system_repos: true,
        });
        hub.seed_default_routines_if_needed()?;
        Ok(hub)
    }

    /// Headless constructor for deterministic unit tests (no WebView required).
    pub fn new_headless(store: Arc<Store>) -> AppResult<Arc<Self>> {
        let hub = Arc::new(Hub {
            app: None,
            store,
            focus: Mutex::new(FocusRuntime::default()),
            git_cache: Mutex::new(None),
            scan_system_repos: false,
        });
        hub.seed_default_routines_if_needed()?;
        Ok(hub)
    }

    /// Warm the git commit cache in the background shortly after launch so
    /// opening the Activity deck in the Island returns in < 2ms.
    pub fn spawn(self: &Arc<Self>) {
        let me = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(4)).await;
            let _ = crate::platform::blocking(move || {
                crate::platform::in_background(|| {
                    let _ = me.git_commits_by_day();
                    Ok(())
                })
            })
            .await;
        });
    }

    fn emit<S: Serialize + Clone>(&self, event: &str, payload: &S) {
        if let Some(app) = &self.app {
            let _ = app.emit(event, payload.clone());
        }
    }

    /// Focus minutes and done tasks are XP: the island and the planner show
    /// the new level, streak and day at once.
    fn growth_changed(&self) {
        if let Some(mentor) = self.app.as_ref().and_then(|app| app.try_state::<Arc<crate::mentor::Mentor>>()) {
            mentor.emit_state();
        }
    }

    // ==================== 1. TASKS & AGENDA ====================

    pub fn tasks_list(&self) -> AppResult<Vec<TaskItem>> {
        self.store.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, notes, scheduled_date, scheduled_time, color, completed, completed_at, created_at
                 FROM hub_tasks
                 ORDER BY completed ASC,
                          CASE WHEN scheduled_date IS NULL THEN 1 ELSE 0 END ASC,
                          scheduled_date ASC,
                          CASE WHEN scheduled_time IS NULL THEN 1 ELSE 0 END ASC,
                          scheduled_time ASC,
                          created_at ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(TaskItem {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    notes: r.get(2)?,
                    scheduled_date: r.get(3)?,
                    scheduled_time: r.get(4)?,
                    color: r.get(5)?,
                    completed: r.get::<_, i64>(6)? != 0,
                    completed_at: r.get(7)?,
                    created_at: r.get(8)?,
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect())
        })
    }

    pub fn task_add(
        &self,
        title: String,
        scheduled_date: Option<String>,
        scheduled_time: Option<String>,
        color: Option<String>,
    ) -> AppResult<TaskItem> {
        self.task_add_with_notes(title, String::new(), scheduled_date, scheduled_time, color)
    }

    pub fn task_add_with_notes(
        &self,
        title: String,
        notes: String,
        scheduled_date: Option<String>,
        scheduled_time: Option<String>,
        color: Option<String>,
    ) -> AppResult<TaskItem> {
        let clean_title = title.trim().to_string();
        if clean_title.is_empty() {
            return Err(AppError::invalid("Название задачи не может быть пустым"));
        }
        let item = TaskItem {
            id: uuid::Uuid::new_v4().to_string(),
            title: clean_title,
            notes: notes.trim().to_string(),
            scheduled_date: normalize_date(scheduled_date)?,
            scheduled_time: normalize_time(scheduled_time)?,
            color: normalize_color(color.as_deref()),
            completed: false,
            completed_at: None,
            created_at: now_rfc3339(),
        };
        self.store.with_conn(|conn| {
            conn.execute(
                "INSERT INTO hub_tasks(id, title, notes, scheduled_date, scheduled_time, color, completed, completed_at, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, NULL, ?7)",
                params![
                    item.id,
                    item.title,
                    item.notes,
                    item.scheduled_date,
                    item.scheduled_time,
                    item.color,
                    item.created_at
                ],
            )?;
            Ok(())
        })?;
        if let Ok(all) = self.tasks_list() {
            self.emit(EVT_TASKS, &all);
        }
        Ok(item)
    }

    fn task_get(&self, id: &str) -> AppResult<TaskItem> {
        self.store.with_conn(|conn| {
            conn.query_row(
                "SELECT id, title, notes, scheduled_date, scheduled_time, color, completed, completed_at, created_at
                 FROM hub_tasks WHERE id = ?1",
                params![id],
                |r| {
                    Ok(TaskItem {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        notes: r.get(2)?,
                        scheduled_date: r.get(3)?,
                        scheduled_time: r.get(4)?,
                        color: r.get(5)?,
                        completed: r.get::<_, i64>(6)? != 0,
                        completed_at: r.get(7)?,
                        created_at: r.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Задача не найдена"))
        })
    }

    pub fn task_toggle(&self, id: &str) -> AppResult<TaskItem> {
        let cur = self.task_get(id)?;
        self.task_set_completed(id, !cur.completed)
    }

    pub fn task_set_completed(&self, id: &str, completed: bool) -> AppResult<TaskItem> {
        let completed_at = if completed { Some(now_rfc3339()) } else { None };
        let changed = self.store.with_conn(|conn| {
            Ok(conn.execute(
                "UPDATE hub_tasks SET completed = ?2, completed_at = ?3 WHERE id = ?1",
                params![id, i64::from(completed), completed_at],
            )?)
        })?;
        if changed == 0 {
            return Err(AppError::not_found("Задача не найдена"));
        }
        let updated = self.task_get(id)?;
        if let Ok(all) = self.tasks_list() {
            self.emit(EVT_TASKS, &all);
        }
        self.growth_changed();
        Ok(updated)
    }

    pub fn task_remove(&self, id: &str) -> AppResult<()> {
        let removed = self.store.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM hub_tasks WHERE id = ?1", params![id])?)
        })?;
        if removed == 0 {
            return Err(AppError::not_found("Задача не найдена"));
        }
        if let Ok(all) = self.tasks_list() {
            self.emit(EVT_TASKS, &all);
        }
        Ok(())
    }

    // ==================== 2. FOCUS SESSION ENGINE ====================

    fn focus_today_stats(&self) -> AppResult<(u32, u32)> {
        let today = today_local_str();
        self.store.with_conn(|conn| {
            let (count, mins): (i64, i64) = conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(minutes), 0) FROM focus_sessions WHERE local_date = ?1",
                params![today],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok((count.max(0) as u32, mins.max(0) as u32))
        })
    }

    fn build_focus_state(&self, active: Option<&ActiveFocus>) -> AppResult<FocusState> {
        let (completed_today, minutes_today) = self.focus_today_stats()?;
        Ok(match active {
            Some(s) => FocusState {
                active: true,
                paused: s.paused,
                task_id: s.task_id.clone(),
                title: s.title.clone(),
                total_sec: s.total_sec,
                remaining_sec: s.remaining_sec,
                started_at: Some(s.started_at.to_rfc3339()),
                ends_at: s.ends_at.map(|d| d.to_rfc3339()),
                completed_today,
                minutes_today,
            },
            None => FocusState {
                active: false,
                paused: false,
                task_id: None,
                title: String::new(),
                total_sec: 0,
                remaining_sec: 0,
                started_at: None,
                ends_at: None,
                completed_today,
                minutes_today,
            },
        })
    }

    pub fn focus_get(&self) -> AppResult<FocusState> {
        let snap = self.focus.lock().current.clone();
        self.build_focus_state(snap.as_ref())
    }

    pub fn focus_start(
        self: &Arc<Self>,
        title: String,
        minutes: u32,
        task_id: Option<String>,
    ) -> AppResult<FocusState> {
        let mins = minutes.clamp(1, 240);
        let total_sec = mins * 60;
        let clean_task_id = task_id.filter(|id| !id.trim().is_empty());
        let resolved_title = if !title.trim().is_empty() {
            title.trim().to_string()
        } else if let Some(ref tid) = clean_task_id {
            self.task_get(tid).map(|t| t.title).unwrap_or_else(|_| "Фокус-сессия".into())
        } else {
            "Фокус-сессия".to_string()
        };

        let now = Utc::now();
        let ends = now + chrono::Duration::seconds(total_sec as i64);
        let active = ActiveFocus {
            task_id: clean_task_id,
            title: resolved_title,
            total_sec,
            remaining_sec: total_sec,
            paused: false,
            started_at: now,
            ends_at: Some(ends),
        };
        let gen = {
            let mut rt = self.focus.lock();
            rt.gen = rt.gen.wrapping_add(1);
            rt.current = Some(active.clone());
            rt.gen
        };
        let state = self.build_focus_state(Some(&active))?;
        self.emit(EVT_FOCUS, &state);
        self.spawn_focus_ticker(gen);
        Ok(state)
    }

    pub fn focus_pause_toggle(self: &Arc<Self>) -> AppResult<FocusState> {
        let (snap, maybe_spawn_gen) = {
            let mut rt = self.focus.lock();
            rt.gen = rt.gen.wrapping_add(1);
            let gen = rt.gen;
            let Some(cur) = rt.current.as_mut() else {
                return Err(AppError::invalid("Нет активной сессии фокуса"));
            };
            cur.paused = !cur.paused;
            if cur.paused {
                cur.ends_at = None;
                (cur.clone(), None)
            } else {
                cur.ends_at = Some(Utc::now() + chrono::Duration::seconds(cur.remaining_sec as i64));
                (cur.clone(), Some(gen))
            }
        };
        let state = self.build_focus_state(Some(&snap))?;
        self.emit(EVT_FOCUS, &state);
        if let Some(gen) = maybe_spawn_gen {
            self.spawn_focus_ticker(gen);
        }
        Ok(state)
    }

    fn record_completed_focus(&self, sess: &ActiveFocus, natural_finish: bool) -> AppResult<()> {
        let elapsed_sec = if natural_finish {
            sess.total_sec
        } else {
            sess.total_sec.saturating_sub(sess.remaining_sec)
        };
        let minutes = if natural_finish {
            (sess.total_sec / 60).max(1)
        } else if elapsed_sec >= 60 {
            ((elapsed_sec + 30) / 60).max(1)
        } else {
            1
        };
        let completed_at = Utc::now();
        let local_date = completed_at.with_timezone(&Local).format("%Y-%m-%d").to_string();
        let id = uuid::Uuid::new_v4().to_string();
        self.store.with_conn(|conn| {
            conn.execute(
                "INSERT INTO focus_sessions(id, task_id, title, planned_sec, elapsed_sec, minutes, started_at, completed_at, local_date)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    sess.task_id,
                    sess.title,
                    sess.total_sec,
                    elapsed_sec,
                    minutes,
                    sess.started_at.to_rfc3339(),
                    completed_at.to_rfc3339(),
                    local_date
                ],
            )?;
            Ok(())
        })?;
        // If a task was linked and is still incomplete, mark it done and notify UI.
        if let Some(ref tid) = sess.task_id {
            if let Ok(task) = self.task_get(tid) {
                if !task.completed {
                    let _ = self.task_set_completed(tid, true);
                }
            }
        }
        self.growth_changed();
        Ok(())
    }

    pub fn focus_complete(&self) -> AppResult<FocusState> {
        let taken = {
            let mut rt = self.focus.lock();
            rt.gen = rt.gen.wrapping_add(1);
            rt.current.take()
        };
        if let Some(sess) = taken {
            self.record_completed_focus(&sess, false)?;
        }
        let state = self.build_focus_state(None)?;
        self.emit(EVT_FOCUS, &state);
        Ok(state)
    }

    pub fn focus_cancel(&self) -> AppResult<FocusState> {
        {
            let mut rt = self.focus.lock();
            rt.gen = rt.gen.wrapping_add(1);
            rt.current = None;
        }
        let state = self.build_focus_state(None)?;
        self.emit(EVT_FOCUS, &state);
        Ok(state)
    }

    /// Advance the active unpaused focus timer by `seconds` (1s in background
    /// ticker; arbitrary steps supported for fast deterministic unit tests).
    /// Returns `Some(true)` if the timer naturally finished on this tick,
    /// `Some(false)` if still ticking, or `None` if ticker generation is stale/paused.
    pub fn focus_step(&self, expected_gen: Option<u64>, seconds: u32) -> AppResult<Option<bool>> {
        enum StepOutcome {
            Running(ActiveFocus),
            Finished(ActiveFocus),
        }
        let outcome = {
            let mut rt = self.focus.lock();
            if let Some(g) = expected_gen {
                if rt.gen != g {
                    return Ok(None);
                }
            }
            let Some(cur) = rt.current.as_mut() else {
                return Ok(None);
            };
            if cur.paused {
                return Ok(None);
            }
            cur.remaining_sec = cur.remaining_sec.saturating_sub(seconds);
            if cur.remaining_sec == 0 {
                rt.gen = rt.gen.wrapping_add(1);
                let finished = rt.current.take().expect("checked Some");
                StepOutcome::Finished(finished)
            } else {
                StepOutcome::Running(cur.clone())
            }
        };

        match outcome {
            StepOutcome::Running(snap) => {
                let state = self.build_focus_state(Some(&snap))?;
                self.emit(EVT_FOCUS, &state);
                Ok(Some(false))
            }
            StepOutcome::Finished(sess) => {
                self.record_completed_focus(&sess, true)?;
                let state = self.build_focus_state(None)?;
                self.emit(EVT_FOCUS, &state);
                if let Some(app) = &self.app {
                    request_tts(app, "Сессия фокуса завершена. Отличная работа.", None, None, false);
                }
                Ok(Some(true))
            }
        }
    }

    fn spawn_focus_ticker(self: &Arc<Self>, gen: u64) {
        if self.app.is_none() {
            return;
        }
        let me = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            // First tick of tokio::time::interval completes immediately; skip it.
            interval.tick().await;
            loop {
                interval.tick().await;
                match me.focus_step(Some(gen), 1) {
                    Ok(Some(false)) => continue,
                    _ => break,
                }
            }
        });
    }

    // ==================== 3. NOTCH HOLDING SHELF (TRAY) ====================

    pub fn shelf_list(&self) -> AppResult<Vec<ShelfItem>> {
        self.store.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, kind, label, value, subtitle, size_bytes, pinned, created_at
                 FROM shelf_items
                 ORDER BY pinned DESC, created_at DESC",
            )?;
            let rows = stmt.query_map([], |r| {
                let kind: String = r.get(1)?;
                let value: String = r.get(3)?;
                let stored_size: Option<i64> = r.get(5)?;
                let (exists, size_bytes) = if kind == "file" {
                    let p = Path::new(&value);
                    let ex = p.exists();
                    let sz = stored_size
                        .map(|n| n.max(0) as u64)
                        .or_else(|| p.metadata().ok().filter(|m| m.is_file()).map(|m| m.len()));
                    (ex, sz)
                } else {
                    (true, stored_size.map(|n| n.max(0) as u64))
                };
                Ok(ShelfItem {
                    id: r.get(0)?,
                    kind,
                    label: r.get(2)?,
                    value,
                    subtitle: r.get(4)?,
                    size_bytes,
                    exists,
                    pinned: r.get::<_, i64>(6)? != 0,
                    created_at: r.get(7)?,
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect())
        })
    }

    pub fn shelf_add_files(&self, paths: Vec<String>) -> AppResult<Vec<ShelfItem>> {
        let mut added = Vec::new();
        for raw in paths {
            let clean = raw.trim().to_string();
            if clean.is_empty() {
                continue;
            }
            let path = Path::new(&clean);
            let exists = path.exists();
            let meta = std::fs::metadata(path).ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            let size_bytes = meta.filter(|m| m.is_file()).map(|m| m.len());
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| clean.clone());
            let parent = path
                .parent()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_default();
            let subtitle = if is_dir {
                if parent.is_empty() { "Папка".to_string() } else { format!("Папка · {parent}") }
            } else {
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_uppercase())
                    .unwrap_or_else(|| "ФАЙЛ".into());
                if parent.is_empty() { ext } else { format!("{ext} · {parent}") }
            };
            let now = now_rfc3339();
            let item = self.store.with_conn(|conn| {
                let existing: Option<(String, bool)> = conn
                    .query_row(
                        "SELECT id, pinned FROM shelf_items WHERE kind = 'file' AND lower(value) = lower(?1) LIMIT 1",
                        params![clean],
                        |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
                    )
                    .optional()?;
                if let Some((id, pinned)) = existing {
                    conn.execute(
                        "UPDATE shelf_items SET label = ?2, subtitle = ?3, size_bytes = ?4, created_at = ?5 WHERE id = ?1",
                        params![id, label, subtitle, size_bytes.map(|n| n as i64), now],
                    )?;
                    Ok(ShelfItem {
                        id,
                        kind: "file".into(),
                        label: label.clone(),
                        value: clean.clone(),
                        subtitle: subtitle.clone(),
                        size_bytes,
                        exists,
                        pinned,
                        created_at: now.clone(),
                    })
                } else {
                    let id = uuid::Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT INTO shelf_items(id, kind, label, value, subtitle, size_bytes, pinned, created_at)
                         VALUES(?1, 'file', ?2, ?3, ?4, ?5, 0, ?6)",
                        params![id, label, clean, subtitle, size_bytes.map(|n| n as i64), now],
                    )?;
                    Ok(ShelfItem {
                        id,
                        kind: "file".into(),
                        label: label.clone(),
                        value: clean.clone(),
                        subtitle: subtitle.clone(),
                        size_bytes,
                        exists,
                        pinned: false,
                        created_at: now.clone(),
                    })
                }
            })?;
            added.push(item);
        }
        if let Ok(all) = self.shelf_list() {
            self.emit(EVT_SHELF, &all);
        }
        Ok(added)
    }

    pub fn shelf_add_text(&self, text: String) -> AppResult<ShelfItem> {
        let clean = text.trim().to_string();
        if clean.is_empty() {
            return Err(AppError::invalid("Текст для полки не может быть пустым"));
        }
        let is_url = !clean.chars().any(char::is_whitespace)
            && (clean.starts_with("https://") || clean.starts_with("http://"))
            && url::Url::parse(&clean).is_ok();
        let (kind, label, subtitle) = if is_url {
            let parsed = url::Url::parse(&clean).expect("checked url");
            let host = parsed.host_str().unwrap_or(&clean);
            let path = parsed.path().trim_matches('/');
            let lbl = if path.is_empty() {
                host.to_string()
            } else {
                let short: String = path.chars().take(28).collect();
                format!("{host}/{short}")
            };
            ("url", lbl, "Ссылка".to_string())
        } else {
            let first_line = clean.lines().find(|l| !l.trim().is_empty()).unwrap_or(&clean).trim();
            let lbl = if first_line.chars().count() > 56 {
                format!("{}…", first_line.chars().take(56).collect::<String>())
            } else {
                first_line.to_string()
            };
            ("text", lbl, format!("{} симв.", clean.chars().count()))
        };
        let size_bytes = Some(clean.len() as u64);
        let now = now_rfc3339();
        let item = self.store.with_conn(|conn| {
            let existing: Option<(String, bool)> = conn
                .query_row(
                    "SELECT id, pinned FROM shelf_items WHERE kind = ?1 AND value = ?2 LIMIT 1",
                    params![kind, clean],
                    |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
                )
                .optional()?;
            if let Some((id, pinned)) = existing {
                conn.execute(
                    "UPDATE shelf_items SET label = ?2, subtitle = ?3, size_bytes = ?4, created_at = ?5 WHERE id = ?1",
                    params![id, label, subtitle, size_bytes.map(|n| n as i64), now],
                )?;
                Ok(ShelfItem {
                    id,
                    kind: kind.into(),
                    label,
                    value: clean,
                    subtitle,
                    size_bytes,
                    exists: true,
                    pinned,
                    created_at: now,
                })
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO shelf_items(id, kind, label, value, subtitle, size_bytes, pinned, created_at)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
                    params![id, kind, label, clean, subtitle, size_bytes.map(|n| n as i64), now],
                )?;
                Ok(ShelfItem {
                    id,
                    kind: kind.into(),
                    label,
                    value: clean,
                    subtitle,
                    size_bytes,
                    exists: true,
                    pinned: false,
                    created_at: now,
                })
            }
        })?;
        if let Ok(all) = self.shelf_list() {
            self.emit(EVT_SHELF, &all);
        }
        Ok(item)
    }

    pub fn shelf_remove(&self, id: &str) -> AppResult<()> {
        self.store.with_conn(|conn| {
            conn.execute("DELETE FROM shelf_items WHERE id = ?1", params![id])?;
            Ok(())
        })?;
        if let Ok(all) = self.shelf_list() {
            self.emit(EVT_SHELF, &all);
        }
        Ok(())
    }

    pub fn shelf_clear(&self) -> AppResult<()> {
        self.store.with_conn(|conn| {
            conn.execute("DELETE FROM shelf_items", [])?;
            Ok(())
        })?;
        if let Ok(all) = self.shelf_list() {
            self.emit(EVT_SHELF, &all);
        }
        Ok(())
    }

    // ==================== 4. REAL ACTIVITY & STREAK HEATMAP ====================

    pub fn activity_get(&self) -> AppResult<ActivityHeatmap> {
        let settings = self.store.settings();
        let user_name = if !settings.persona.user_name.trim().is_empty() {
            settings.persona.user_name.trim().to_string()
        } else {
            std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Пользователь".to_string())
        };

        let today = Local::now().date_naive();
        let start_date = today - chrono::Duration::days(363);
        let start_str = start_date.format("%Y-%m-%d").to_string();
        // Slight padding for UTC vs Local timestamps near the 364-day boundary.
        let cutoff_iso = (Utc::now() - chrono::Duration::days(366)).to_rfc3339();

        let mut map: HashMap<NaiveDate, RawDayCounts> = HashMap::new();

        self.store.with_conn(|conn| {
            // 1) Focus sessions
            {
                let mut stmt = conn.prepare(
                    "SELECT local_date, SUM(minutes), COUNT(*) FROM focus_sessions WHERE local_date >= ?1 GROUP BY local_date",
                )?;
                let rows = stmt.query_map(params![start_str], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
                })?;
                for row in rows.flatten() {
                    if let Ok(d) = NaiveDate::parse_from_str(&row.0, "%Y-%m-%d") {
                        let entry = map.entry(d).or_default();
                        entry.focus_minutes += row.1.max(0) as u32;
                        entry.focus_sessions += row.2.max(0) as u32;
                    }
                }
            }
            // 2) Completed Hub tasks
            {
                let mut stmt = conn.prepare(
                    "SELECT completed_at FROM hub_tasks WHERE completed = 1 AND completed_at IS NOT NULL AND completed_at >= ?1",
                )?;
                let rows = stmt.query_map(params![cutoff_iso], |r| r.get::<_, String>(0))?;
                for ts in rows.flatten() {
                    if let Some(d) = parse_local_date(&ts) {
                        map.entry(d).or_default().tasks_done += 1;
                    }
                }
            }
            // 3) Audit log entries (agent actions)
            {
                let mut stmt = conn.prepare("SELECT at FROM audit_log WHERE at >= ?1")?;
                let rows = stmt.query_map(params![cutoff_iso], |r| r.get::<_, String>(0))?;
                for ts in rows.flatten() {
                    if let Some(d) = parse_local_date(&ts) {
                        map.entry(d).or_default().agent_actions += 1;
                    }
                }
            }
            // 4) User messages in conversations
            {
                let mut stmt = conn.prepare(
                    "SELECT created_at, payload FROM messages WHERE tool_title IS NULL AND created_at >= ?1",
                )?;
                let rows = stmt.query_map(params![cutoff_iso], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                for (ts, payload) in rows.flatten() {
                    if payload.contains("\"role\":\"user\"") && !payload.contains("[tool-image]") {
                        if let Some(d) = parse_local_date(&ts) {
                            map.entry(d).or_default().agent_actions += 1;
                        }
                    }
                }
            }
            Ok(())
        })?;

        // 5) Real local git commits (cached in memory for 5 minutes)
        for (date, commits) in self.git_commits_by_day() {
            map.entry(date).or_default().git_commits += commits;
        }

        Ok(build_heatmap_from_daily(user_name, today, &map))
    }

    fn git_commits_by_day(&self) -> HashMap<NaiveDate, u32> {
        if let Some((at, ref cached)) = *self.git_cache.lock() {
            if at.elapsed() < GIT_CACHE_TTL {
                return cached.clone();
            }
        }
        let roots = self.discover_git_candidates();
        let fresh = collect_git_commits_from_roots(&roots);
        *self.git_cache.lock() = Some((Instant::now(), fresh.clone()));
        fresh
    }

    fn discover_git_candidates(&self) -> Vec<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        for r in self.store.settings().agent.project_roots {
            if !r.trim().is_empty() {
                candidates.push(PathBuf::from(r));
            }
        }
        if !self.scan_system_repos {
            return candidates;
        }
        for r in crate::tools::vscode_recent() {
            candidates.push(PathBuf::from(r));
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd);
        }
        if let Some(home) = dirs::home_dir() {
            for sub in ["Documents", "projects", "source/repos", "dev", "code"] {
                scan_git_dirs(&home.join(sub), 2, &mut candidates, 28);
            }
        }
        candidates
    }

    // ==================== 5. ROUTINES / PLAYBOOKS ====================

    fn seed_default_routines_if_needed(&self) -> AppResult<()> {
        if self.store.kv_get("routines_seeded").is_some() {
            return Ok(());
        }
        let count: i64 = self.store.with_conn(|conn| {
            Ok(conn.query_row("SELECT COUNT(*) FROM routines", [], |r| r.get(0))?)
        })?;
        if count == 0 {
            let defaults = [
                (
                    "routine-deep-work",
                    "Глубокая работа",
                    "Фокус 25 мин · порядок в окнах · тишина",
                    "target",
                    "Включи режим глубокой работы: составь план (mission_plan), сверни лишние окна кроме основного рабочего окна и браузера (window_layout focus_clean), поставь комфортную громкость 20% и запусти сессию фокуса на 25 минут (focus_session).",
                    true,
                ),
                (
                    "routine-morning-briefing",
                    "Утренний брифинг",
                    "Задачи на день · состояние ПК · сводка вслух",
                    "sun",
                    "Проведи короткий утренний брифинг: проверь мои задачи в хабе (tasks_manage), активные окна и состояние системы (system_diagnostics), собери чёткую сводку на день и озвучь её вслух (speak_aloud).",
                    false,
                ),
                (
                    "routine-window-order",
                    "Порядок в окнах",
                    "Два главных окна рядом · остальное свернуть",
                    "layout",
                    "Наведи порядок на рабочем столе: сохрани текущее расположение окон (window_layout snapshot), сверни второстепенные окна (window_layout focus_clean) и разложи два последних рабочих окна рядом 50-50 (window_layout tile_pair).",
                    true,
                ),
                (
                    "routine-pc-diagnostics",
                    "Диагностика ПК",
                    "CPU, RAM, диск C:, батарея и тяжёлые процессы",
                    "activity",
                    "Выполни диагностику ПК через system_diagnostics: проверь загрузку CPU и RAM, свободное место на диске C:, статус питания/батареи и топ-5 процессов по памяти, и дай краткое заключение.",
                    false,
                ),
            ];
            let now = now_rfc3339();
            self.store.with_conn(|conn| {
                for (idx, (id, title, subtitle, icon, prompt, autonomy)) in defaults.iter().enumerate() {
                    conn.execute(
                        "INSERT OR IGNORE INTO routines(id, title, subtitle, icon, prompt, autonomy, sort_order, created_at)
                         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![id, title, subtitle, icon, prompt, i64::from(*autonomy), idx as i64, now],
                    )?;
                }
                Ok(())
            })?;
        }
        let _ = self.store.kv_set("routines_seeded", "1");
        Ok(())
    }

    pub fn routines_list(&self) -> AppResult<Vec<RoutineItem>> {
        self.store.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, subtitle, icon, prompt, autonomy
                 FROM routines
                 ORDER BY sort_order ASC, created_at ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(RoutineItem {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    subtitle: r.get(2)?,
                    icon: r.get(3)?,
                    prompt: r.get(4)?,
                    autonomy: r.get::<_, i64>(5)? != 0,
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect())
        })
    }

    pub fn routine_get(&self, id: &str) -> AppResult<RoutineItem> {
        self.store.with_conn(|conn| {
            conn.query_row(
                "SELECT id, title, subtitle, icon, prompt, autonomy FROM routines WHERE id = ?1",
                params![id],
                |r| {
                    Ok(RoutineItem {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        subtitle: r.get(2)?,
                        icon: r.get(3)?,
                        prompt: r.get(4)?,
                        autonomy: r.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("Сценарий не найден"))
        })
    }

    pub fn routine_save(
        &self,
        id: Option<String>,
        title: String,
        subtitle: String,
        icon: String,
        prompt: String,
        autonomy: bool,
    ) -> AppResult<RoutineItem> {
        let clean_title = title.trim().to_string();
        let clean_prompt = prompt.trim().to_string();
        if clean_title.is_empty() {
            return Err(AppError::invalid("Название сценария не может быть пустым"));
        }
        if clean_prompt.is_empty() {
            return Err(AppError::invalid("Текст команды сценария не может быть пустым"));
        }
        let clean_icon = if icon.trim().is_empty() { "zap".to_string() } else { icon.trim().to_string() };
        let clean_sub = subtitle.trim().to_string();
        let routine_id = id.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let now = now_rfc3339();

        self.store.with_conn(|conn| {
            let existing_order: Option<i64> = conn
                .query_row("SELECT sort_order FROM routines WHERE id = ?1", params![routine_id], |r| r.get(0))
                .optional()?;
            let sort_order = match existing_order {
                Some(o) => o,
                None => conn
                    .query_row("SELECT COALESCE(MAX(sort_order), -1) + 1 FROM routines", [], |r| r.get::<_, i64>(0))
                    .unwrap_or(0),
            };
            conn.execute(
                "INSERT INTO routines(id, title, subtitle, icon, prompt, autonomy, sort_order, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                   title = excluded.title,
                   subtitle = excluded.subtitle,
                   icon = excluded.icon,
                   prompt = excluded.prompt,
                   autonomy = excluded.autonomy",
                params![routine_id, clean_title, clean_sub, clean_icon, clean_prompt, i64::from(autonomy), sort_order, now],
            )?;
            Ok(())
        })?;
        let saved = self.routine_get(&routine_id)?;
        if let Ok(all) = self.routines_list() {
            self.emit(EVT_ROUTINES, &all);
        }
        Ok(saved)
    }

    pub fn routine_remove(&self, id: &str) -> AppResult<()> {
        let removed = self.store.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM routines WHERE id = ?1", params![id])?)
        })?;
        if removed == 0 {
            return Err(AppError::not_found("Сценарий не найден"));
        }
        if let Ok(all) = self.routines_list() {
            self.emit(EVT_ROUTINES, &all);
        }
        Ok(())
    }
}

fn scan_git_dirs(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, max: usize) {
    if out.len() >= max {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        if out.len() >= max {
            break;
        }
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "appdata" {
            continue;
        }
        if p.join(".git").exists() {
            out.push(p);
        } else if depth > 0 {
            scan_git_dirs(&p, depth - 1, out, max);
        }
    }
}

/// Resolve any folder inside a git checkout (or worktree) to `(working_root, common_git_key)`
/// so multiple worktrees of the same repository are never counted twice.
fn resolve_git_repo_key(path: &Path) -> Option<(PathBuf, String)> {
    let mut cur = Some(path);
    while let Some(dir) = cur {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            let canon = std::fs::canonicalize(&dot_git).unwrap_or(dot_git);
            return Some((dir.to_path_buf(), canon.to_string_lossy().to_lowercase()));
        }
        if dot_git.is_file() {
            // Worktree `.git` file: `gitdir: C:/.../.git/worktrees/xyz`
            if let Ok(raw) = std::fs::read_to_string(&dot_git) {
                if let Some(gd) = raw.lines().find_map(|l| l.strip_prefix("gitdir:")) {
                    let gd_path = PathBuf::from(gd.trim());
                    let resolved = if gd_path.is_absolute() { gd_path } else { dir.join(gd_path) };
                    // `.../.git/worktrees/<name>` -> common dir is parent.parent (`.../.git`)
                    let common = resolved
                        .parent()
                        .and_then(|p| {
                            if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("worktrees")) {
                                p.parent().map(Path::to_path_buf)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(resolved);
                    let canon = std::fs::canonicalize(&common).unwrap_or(common);
                    return Some((dir.to_path_buf(), canon.to_string_lossy().to_lowercase()));
                }
            }
        }
        cur = dir.parent();
    }
    None
}

pub fn collect_git_commits_from_roots(candidates: &[PathBuf]) -> HashMap<NaiveDate, u32> {
    let mut seen_repos = HashSet::new();
    let mut repos = Vec::new();
    for c in candidates {
        if let Some((repo_dir, key)) = resolve_git_repo_key(c) {
            if seen_repos.insert(key) {
                repos.push(repo_dir);
            }
        }
    }
    let mut counts: HashMap<NaiveDate, u32> = HashMap::new();
    for repo in repos {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("-C")
            .arg(&repo)
            .args(["log", "--since=365.days", "--format=%cs"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if let Ok(d) = NaiveDate::parse_from_str(line.trim(), "%Y-%m-%d") {
                        *counts.entry(d).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    counts
}

/// Pure, deterministic 364-day (52-week) heatmap & streak builder.
pub fn build_heatmap_from_daily(
    user_name: String,
    today: NaiveDate,
    raw: &HashMap<NaiveDate, RawDayCounts>,
) -> ActivityHeatmap {
    let start = today - chrono::Duration::days(363);
    let mut days = Vec::with_capacity(364);
    for offset in 0..364 {
        let d = start + chrono::Duration::days(offset);
        let c = raw.get(&d).cloned().unwrap_or_default();
        let focus_units = if c.focus_sessions > 0 {
            c.focus_sessions
        } else {
            u32::from(c.focus_minutes > 0)
        };
        let count = c.git_commits + c.tasks_done + c.agent_actions + focus_units;
        days.push(ActivityDay {
            date: d.format("%Y-%m-%d").to_string(),
            count,
            level: 0,
            focus_minutes: c.focus_minutes,
            tasks_done: c.tasks_done,
            agent_actions: c.agent_actions,
            git_commits: c.git_commits,
        });
    }

    // Quartile levels (1..=4) over non-zero daily totals.
    let mut nonzero: Vec<u32> = days.iter().map(|d| d.count).filter(|&n| n > 0).collect();
    nonzero.sort_unstable();
    if let Some(&max_val) = nonzero.last() {
        let (q1, q2, q3) = if max_val <= 4 {
            (1, 2, 3)
        } else {
            let pick = |num: usize, den: usize| nonzero[(nonzero.len().saturating_sub(1) * num) / den];
            let q1 = pick(1, 4).max(1);
            let q2 = pick(2, 4).max(q1 + 1);
            let q3 = pick(3, 4).max(q2 + 1);
            (q1, q2, q3)
        };
        for day in &mut days {
            day.level = if day.count == 0 {
                0
            } else if day.count <= q1 {
                1
            } else if day.count <= q2 {
                2
            } else if day.count <= q3 {
                3
            } else {
                4
            };
        }
    }

    let total_year: u32 = days.iter().map(|d| d.count).sum();

    // Longest streak across the 364-day window.
    let mut longest_streak = 0u32;
    let mut run = 0u32;
    for d in &days {
        if d.count > 0 {
            run += 1;
            longest_streak = longest_streak.max(run);
        } else {
            run = 0;
        }
    }

    // Current streak: counts backwards from today, or from yesterday if today has 0 so far.
    let mut current_streak = 0u32;
    let anchor = if days.last().is_some_and(|d| d.count > 0) {
        Some(days.len() - 1)
    } else if days.len() >= 2 && days[days.len() - 2].count > 0 {
        Some(days.len() - 2)
    } else {
        None
    };
    if let Some(end_idx) = anchor {
        for d in days[..=end_idx].iter().rev() {
            if d.count > 0 {
                current_streak += 1;
            } else {
                break;
            }
        }
    }

    ActivityHeatmap {
        user_name,
        total_year,
        current_streak,
        longest_streak,
        days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::{ChatMessage, Role};

    fn test_hub() -> Arc<Hub> {
        let store = Arc::new(Store::in_memory().unwrap());
        Hub::new_headless(store).unwrap()
    }

    #[test]
    fn tasks_crud_ordering_and_date_normalization() {
        let hub = test_hub();
        assert!(hub.tasks_list().unwrap().is_empty());

        let t1 = hub
            .task_add("Созвон с командой".into(), Some("2026-11-20".into()), Some("14:30".into()), Some("coral".into()))
            .unwrap();
        let t2 = hub
            .task_add("Код-ревью".into(), Some("today".into()), Some("09:00".into()), Some("emerald".into()))
            .unwrap();
        let t3 = hub.task_add("Без даты".into(), None, None, Some("unknown-color".into())).unwrap();
        assert_eq!(t3.color, "blue");

        let list = hub.tasks_list().unwrap();
        assert_eq!(list.len(), 3);
        // Scheduled items appear before unscheduled ones.
        assert_eq!(list[2].id, t3.id);

        // Toggle completion moves task to bottom and stamps completedAt.
        let done = hub.task_toggle(&t2.id).unwrap();
        assert!(done.completed);
        assert!(done.completed_at.is_some());

        let list_after = hub.tasks_list().unwrap();
        assert_eq!(list_after.last().unwrap().id, t2.id);

        // Toggle back to incomplete clears completedAt.
        let reopened = hub.task_toggle(&t2.id).unwrap();
        assert!(!reopened.completed);
        assert!(reopened.completed_at.is_none());

        // Remove task.
        hub.task_remove(&t1.id).unwrap();
        assert_eq!(hub.tasks_list().unwrap().len(), 2);
        assert!(hub.task_remove("non-existent").is_err());
    }

    #[test]
    fn focus_countdown_pause_completion_and_linked_task() {
        let hub = test_hub();
        let task = hub.task_add("Написать ядро хаба".into(), None, None, Some("amber".into())).unwrap();

        let idle = hub.focus_get().unwrap();
        assert!(!idle.active);
        assert_eq!(idle.completed_today, 0);

        let started = hub.focus_start("".into(), 25, Some(task.id.clone())).unwrap();
        assert!(started.active);
        assert!(!started.paused);
        assert_eq!(started.title, "Написать ядро хаба");
        assert_eq!(started.total_sec, 1500);
        assert_eq!(started.remaining_sec, 1500);
        assert!(started.ends_at.is_some());

        // Tick 600s -> 900s remaining.
        assert_eq!(hub.focus_step(None, 600).unwrap(), Some(false));
        assert_eq!(hub.focus_get().unwrap().remaining_sec, 900);

        // Pause -> ticking becomes a no-op and ends_at clears.
        let paused = hub.focus_pause_toggle().unwrap();
        assert!(paused.paused);
        assert!(paused.ends_at.is_none());
        assert_eq!(hub.focus_step(None, 60).unwrap(), None);
        assert_eq!(hub.focus_get().unwrap().remaining_sec, 900);

        // Resume -> countdown continues to 0 and auto-logs session + completes linked task!
        let resumed = hub.focus_pause_toggle().unwrap();
        assert!(!resumed.paused);
        assert!(resumed.ends_at.is_some());
        assert_eq!(hub.focus_step(None, 900).unwrap(), Some(true));

        let after = hub.focus_get().unwrap();
        assert!(!after.active);
        assert_eq!(after.completed_today, 1);
        assert_eq!(after.minutes_today, 25);
        assert!(hub.tasks_list().unwrap()[0].completed);

        // Manual cancel does not record a session.
        hub.focus_start("Черновик".into(), 15, None).unwrap();
        let cancelled = hub.focus_cancel().unwrap();
        assert!(!cancelled.active);
        assert_eq!(cancelled.completed_today, 1);
    }

    #[test]
    fn shelf_tracks_files_urls_text_and_existence() {
        let hub = test_hub();
        let cargo_toml = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("nonexistent_12345.bin");

        let added = hub
            .shelf_add_files(vec![cargo_toml.display().to_string(), missing.display().to_string()])
            .unwrap();
        assert_eq!(added.len(), 2);
        assert!(added[0].exists);
        assert!(added[0].size_bytes.unwrap_or(0) > 50);
        assert!(!added[1].exists);

        let url_item = hub.shelf_add_text("https://github.com/tauri-apps/tauri".into()).unwrap();
        assert_eq!(url_item.kind, "url");
        assert!(url_item.label.contains("github.com"));

        let note = hub.shelf_add_text("Важная заметка для релиза".into()).unwrap();
        assert_eq!(note.kind, "text");

        assert_eq!(hub.shelf_list().unwrap().len(), 4);
        hub.shelf_remove(&note.id).unwrap();
        assert_eq!(hub.shelf_list().unwrap().len(), 3);
        hub.shelf_clear().unwrap();
        assert!(hub.shelf_list().unwrap().is_empty());
    }

    #[test]
    fn activity_heatmap_streaks_quartiles_and_real_sqlite_aggregation() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        let mut raw = HashMap::new();
        // 5-day historical streak in October
        for d in 1..=5 {
            raw.insert(
                NaiveDate::from_ymd_opt(2026, 10, d).unwrap(),
                RawDayCounts { git_commits: d * 3, ..Default::default() },
            );
        }
        // 3-day streak ending yesterday (today has 0 so far -> streak should still be 3!)
        for back in 1..=3 {
            raw.insert(
                today - chrono::Duration::days(back),
                RawDayCounts { tasks_done: 1, focus_sessions: 1, focus_minutes: 25, ..Default::default() },
            );
        }
        let hm = build_heatmap_from_daily("Павел".into(), today, &raw);
        assert_eq!(hm.user_name, "Павел");
        assert_eq!(hm.days.len(), 364);
        assert_eq!(hm.current_streak, 3);
        assert_eq!(hm.longest_streak, 5);
        assert!(hm.days.iter().any(|d| d.level == 4));
        assert_eq!(hm.days.last().unwrap().level, 0);

        // Now test real SQLite aggregation end-to-end.
        let store = Arc::new(Store::in_memory().unwrap());
        let mut settings = store.settings();
        settings.persona.user_name = "Алексей".into();
        store.save_settings(&settings).unwrap();
        store.create_conversation("c1").unwrap();
        store.append_message("c1", &ChatMessage::text(Role::User, "запусти диагностику"), None, None).unwrap();
        store.audit(Some("t1"), "window_layout", "moderate", "{}", "executed", Some("ok"));

        let hub = Hub::new_headless(store).unwrap();
        let task = hub.task_add("Проверить сборку".into(), None, None, None).unwrap();
        hub.task_toggle(&task.id).unwrap();
        hub.focus_start("Спринт".into(), 30, None).unwrap();
        hub.focus_complete().unwrap();

        let live = hub.activity_get().unwrap();
        assert_eq!(live.user_name, "Алексей");
        let last = live.days.last().unwrap();
        assert_eq!(last.tasks_done, 1);
        assert_eq!(last.agent_actions, 2);
        assert!(last.focus_minutes >= 1);
        assert!(last.count >= 4);
        assert!(live.current_streak >= 1);
    }

    #[test]
    fn routines_seed_four_russian_playbooks_once_and_support_crud() {
        let store = Arc::new(Store::in_memory().unwrap());
        let hub = Hub::new_headless(store.clone()).unwrap();
        let seeded = hub.routines_list().unwrap();
        assert_eq!(seeded.len(), 4);
        let titles: Vec<&str> = seeded.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Глубокая работа", "Утренний брифинг", "Порядок в окнах", "Диагностика ПК"]);

        let custom = hub
            .routine_save(None, "Чистый стол".into(), "Свернуть всё".into(), "sparkles".into(), "Сверни лишние окна".into(), true)
            .unwrap();
        assert_eq!(hub.routines_list().unwrap().len(), 5);

        let edited = hub
            .routine_save(Some(custom.id.clone()), "Чистый экран".into(), "Обновлено".into(), "sparkles".into(), "Сверни всё".into(), false)
            .unwrap();
        assert_eq!(edited.title, "Чистый экран");
        assert!(!edited.autonomy);

        // Removing all routines does NOT re-seed them on re-open.
        for r in hub.routines_list().unwrap() {
            hub.routine_remove(&r.id).unwrap();
        }
        assert!(hub.routines_list().unwrap().is_empty());
        let hub_reopened = Hub::new_headless(store).unwrap();
        assert!(hub_reopened.routines_list().unwrap().is_empty());
    }

    #[test]
    fn real_git_commit_collector_deduplicates_worktree_and_reads_commits() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let parent = manifest.parent().unwrap().to_path_buf();
        // Passing both parent and src-tauri subdir must resolve to the same repo once.
        let single = collect_git_commits_from_roots(&[manifest.clone()]);
        let dup = collect_git_commits_from_roots(&[manifest, parent]);
        assert_eq!(single, dup);
        assert!(single.values().sum::<u32>() > 0, "Expected real git commits in this repo");
    }
}
