//! The stopwatch: start, pause, laps, reset. Kept in memory only — a
//! stopwatch measures something happening now, not across restarts. The
//! island ticks it from `startedAt` + `accumulatedMs`, so nothing is sent
//! while it runs.

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const EVT_CHANGED: &str = "stopwatch:changed";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    /// When the current run started (running only).
    pub started_at: Option<DateTime<Utc>>,
    /// Time from earlier runs (before pauses).
    pub accumulated_ms: u64,
    /// Elapsed time at each lap mark.
    pub laps: Vec<u64>,
    /// Shown on the island: started at least once and not reset.
    pub active: bool,
}

impl Status {
    pub fn elapsed_ms(&self) -> u64 {
        self.accumulated_ms + self.started_at.map(|s| (Utc::now() - s).num_milliseconds().max(0) as u64).unwrap_or(0)
    }
}

pub struct Stopwatch {
    app: AppHandle,
    state: Mutex<Status>,
}

impl Stopwatch {
    pub fn new(app: AppHandle) -> Self {
        Stopwatch { app, state: Mutex::new(Status::default()) }
    }

    pub fn status(&self) -> Status {
        self.state.lock().clone()
    }

    fn changed(&self) -> Status {
        let s = self.status();
        let _ = self.app.emit(EVT_CHANGED, &s);
        s
    }

    pub fn start(&self) -> Status {
        {
            let mut s = self.state.lock();
            if !s.running {
                s.running = true;
                s.active = true;
                s.started_at = Some(Utc::now());
            }
        }
        self.changed()
    }

    pub fn pause(&self) -> Status {
        {
            let mut s = self.state.lock();
            if s.running {
                s.accumulated_ms = s.elapsed_ms();
                s.running = false;
                s.started_at = None;
            }
        }
        self.changed()
    }

    pub fn lap(&self) -> Status {
        {
            let mut s = self.state.lock();
            if s.active {
                let at = s.elapsed_ms();
                s.laps.push(at);
            }
        }
        self.changed()
    }

    pub fn reset(&self) -> Status {
        *self.state.lock() = Status::default();
        self.changed()
    }
}

/// «1 минута 23 секунды», «2 часа 5 минут» — for speech.
pub fn spoken(ms: u64) -> String {
    let secs = ms / 1000;
    if secs == 0 {
        return format!("{} десятых секунды", (ms / 100).max(1));
    }
    crate::reminders::spoken_duration(secs)
}
