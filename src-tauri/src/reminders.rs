//! Timers and reminders. Stored in SQLite, so they survive a restart; a
//! single task sleeps until the nearest one. Each fires once (`fired_at`),
//! including those missed while the PC slept or JARVIS was closed — they
//! fire at the next start. The island shows the alert until «Готово» or
//! «Позже»; with an open voice session JARVIS also says it.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::Reminder;

pub const EVT_CHANGED: &str = "reminders:changed";
pub const EVT_FIRED: &str = "reminder:fired";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Fired {
    id: i64,
    kind: String,
    text: String,
}

pub struct Reminders {
    app: AppHandle,
    wake: Notify,
}

/// «5 минут», «полчаса», «1 час 20 минут», «90 секунд» → seconds and the
/// words after the duration. Only the start of `s` is read.
pub fn parse_duration(s: &str) -> Option<(u64, String)> {
    let words: Vec<String> = s.to_lowercase().replace('ё', "е").split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).collect();
    let mut i = 0;
    let mut total = 0u64;
    let mut any = false;
    while i < words.len() {
        let w = words[i].as_str();
        let special = match w {
            "полчаса" => Some(1800),
            "полминуты" => Some(30),
            "полтора" | "полторы" => match words.get(i + 1).map(String::as_str) {
                Some(u) if u.starts_with("час") => Some(5400),
                Some(u) if u.starts_with("минут") => Some(90),
                _ => None,
            },
            _ => None,
        };
        if let Some(sec) = special {
            total += sec;
            any = true;
            i += if w.starts_with("полтор") { 2 } else { 1 };
            continue;
        }
        let (n, used) = match number(&words[i..]) {
            Some(x) => x,
            None if unit(w).is_some() => (1, 0), // «через час», «через минуту»
            None => break,
        };
        let Some(u) = words.get(i + used).and_then(|u| unit(u)) else { break };
        total += n * u;
        any = true;
        i += used + 1;
        if words.get(i).is_some_and(|w| w == "и") {
            i += 1;
        }
    }
    (any && total > 0).then(|| (total, words[i..].join(" ")))
}

fn unit(w: &str) -> Option<u64> {
    if w.starts_with("сек") {
        Some(1)
    } else if w.starts_with("мин") {
        Some(60)
    } else if w.starts_with("час") {
        Some(3600)
    } else {
        None
    }
}

/// A number in digits or Russian words (up to 999): value and words used.
fn number(w: &[String]) -> Option<(u64, usize)> {
    if let Some(n) = w.first().and_then(|x| x.parse::<u64>().ok()) {
        return Some((n, 1));
    }
    const ONES: &[&str] = &["ноль", "одну", "две", "три", "четыре", "пять", "шесть", "семь", "восемь", "девять", "десять", "одиннадцать", "двенадцать", "тринадцать", "четырнадцать", "пятнадцать", "шестнадцать", "семнадцать", "восемнадцать", "девятнадцать"];
    const TENS: &[&str] = &["двадцать", "тридцать", "сорок", "пятьдесят", "шестьдесят", "семьдесят", "восемьдесят", "девяносто"];
    const HUNDREDS: &[&str] = &["сто", "двести", "триста", "четыреста", "пятьсот", "шестьсот", "семьсот", "восемьсот", "девятьсот"];
    let small = |x: &str| match x {
        "один" | "одна" => Some(1),
        "два" => Some(2),
        _ => ONES.iter().position(|o| *o == x).map(|p| p as u64),
    };
    let mut n = 0;
    let mut used = 0;
    if let Some(h) = w.first().and_then(|x| HUNDREDS.iter().position(|o| o == x)) {
        n += (h as u64 + 1) * 100;
        used = 1;
    }
    if let Some(t) = w.get(used).and_then(|x| TENS.iter().position(|o| o == x)) {
        n += (t as u64 + 2) * 10;
        used += 1;
        if let Some(o) = w.get(used).and_then(|x| small(x)).filter(|o| (1..10).contains(o)) {
            n += o;
            used += 1;
        }
    } else if let Some(o) = w.get(used).and_then(|x| small(x)) {
        n += o;
        used += 1;
    }
    (used > 0).then_some((n, used))
}

/// «через 5 минут» in speech: «5 минут», «1 ч 20 мин».
pub fn spoken_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(crate::filekit::plural(h as usize, "час", "часа", "часов"));
    }
    if m > 0 {
        parts.push(crate::filekit::plural(m as usize, "минута", "минуты", "минут"));
    }
    if s > 0 && h == 0 {
        parts.push(crate::filekit::plural(s as usize, "секунда", "секунды", "секунд"));
    }
    if parts.is_empty() { "0 секунд".into() } else { parts.join(" ") }
}

/// «2026-10-23 18:30» or «18:30» (today, or tomorrow if passed) in local time.
pub fn parse_local(at: &str) -> Option<DateTime<Utc>> {
    let at = at.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M") {
        return Local.from_local_datetime(&dt).single().map(|d| d.with_timezone(&Utc));
    }
    let t = chrono::NaiveTime::parse_from_str(at, "%H:%M").ok()?;
    let now = Local::now();
    let mut d = now.date_naive().and_time(t);
    if d <= now.naive_local() {
        d += chrono::Duration::days(1);
    }
    Local.from_local_datetime(&d).single().map(|d| d.with_timezone(&Utc))
}

pub fn local_hm(due: &DateTime<Utc>) -> String {
    due.with_timezone(&Local).format("%H:%M").to_string()
}

impl Reminders {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Reminders { app, wake: Notify::new() })
    }

    fn store(&self) -> Arc<crate::store::Store> {
        self.app.state::<AppState>().store.clone()
    }

    pub fn list(&self) -> Vec<Reminder> {
        self.store().open_reminders().unwrap_or_default()
    }

    fn changed(&self) {
        let _ = self.app.emit(EVT_CHANGED, self.list());
        self.wake.notify_one();
    }

    pub fn create(&self, kind: &str, text: &str, due: DateTime<Utc>) -> AppResult<Reminder> {
        if due <= Utc::now() - chrono::Duration::seconds(5) {
            return Err(AppError::invalid("Это время уже прошло"));
        }
        let kind = if kind == "timer" { "timer" } else { "reminder" };
        let id = self.store().add_reminder(kind, text.trim(), &due.to_rfc3339())?;
        self.changed();
        self.list().into_iter().find(|r| r.id == id).ok_or_else(|| AppError::internal("Напоминание не сохранилось"))
    }

    pub fn done(&self, id: i64) -> AppResult<()> {
        self.store().finish_reminder(id)?;
        self.changed();
        Ok(())
    }

    pub fn snooze(&self, id: i64, minutes: u32) -> AppResult<()> {
        let due = Utc::now() + chrono::Duration::minutes(minutes.clamp(1, 24 * 60) as i64);
        if !self.store().reschedule_reminder(id, &due.to_rfc3339())? {
            return Err(AppError::not_found("Напоминание не найдено"));
        }
        self.changed();
        Ok(())
    }

    fn fire(&self, r: &Reminder) {
        match self.store().mark_fired(r.id) {
            Ok(true) => {}
            _ => return,
        }
        tracing::info!(id = r.id, kind = %r.kind, "reminder fired");
        let _ = self.app.emit(EVT_FIRED, Fired { id: r.id, kind: r.kind.clone(), text: r.text.clone() });
        let line = match (r.kind.as_str(), r.text.is_empty()) {
            ("timer", true) => "Время вышло.".to_string(),
            ("timer", false) => format!("Таймер «{}» закончился.", r.text),
            _ => format!("Напоминаю: {}", r.text),
        };
        self.app.state::<Arc<crate::voice::VoiceManager>>().say(&line);
    }

    /// The scheduler: fire what is due, sleep until the next one (at most a
    /// minute, so clock changes and sleep/resume are noticed).
    pub fn spawn(self: &Arc<Self>) {
        let me = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let _ = me.app.emit(EVT_CHANGED, me.list());
            loop {
                let now = Utc::now();
                let mut next: Option<DateTime<Utc>> = None;
                let mut fired_any = false;
                for r in me.list().iter().filter(|r| r.fired_at.is_none()) {
                    let Ok(due) = DateTime::parse_from_rfc3339(&r.due_at).map(|d| d.with_timezone(&Utc)) else { continue };
                    if due <= now {
                        me.fire(r);
                        fired_any = true;
                    } else if next.is_none_or(|n| due < n) {
                        next = Some(due);
                    }
                }
                if fired_any {
                    let _ = me.app.emit(EVT_CHANGED, me.list());
                }
                let wait = next
                    .map(|n| (n - Utc::now()).to_std().unwrap_or(Duration::ZERO))
                    .unwrap_or(Duration::from_secs(60))
                    .min(Duration::from_secs(60));
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = me.wake.notified() => {}
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_in_words_and_digits() {
        assert_eq!(parse_duration("5 минут"), Some((300, String::new())));
        assert_eq!(parse_duration("двадцать пять минут"), Some((1500, String::new())));
        assert_eq!(parse_duration("полчаса позвонить маме"), Some((1800, "позвонить маме".into())));
        assert_eq!(parse_duration("час про созвон"), Some((3600, "про созвон".into())));
        assert_eq!(parse_duration("1 час 20 минут"), Some((4800, String::new())));
        assert_eq!(parse_duration("полтора часа"), Some((5400, String::new())));
        assert_eq!(parse_duration("90 секунд"), Some((90, String::new())));
        assert_eq!(parse_duration("одну минуту"), Some((60, String::new())));
        assert_eq!(parse_duration("завтра в десять"), None);
        assert_eq!(spoken_duration(4800), "1 час 20 минут");
        assert_eq!(spoken_duration(45), "45 секунд");
    }
}
