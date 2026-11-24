//! IPC for the planner window and its Google Calendar sync.

use std::sync::Arc;

use chrono::{Local, NaiveDate};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::error::{AppError, AppResult};
use crate::planner::google::{self, SyncStatus};
use crate::planner::suggest::{self, Suggestion};
use crate::planner::{CalEvent, EventInput, EventPatch, Planner};

pub const PLANNER_LABEL: &str = "planner";

/// Open the planner (or bring it forward), optionally on a date.
#[tauri::command]
pub async fn planner_open(app: AppHandle, date: Option<String>) -> AppResult<()> {
    open_window(&app, date)
}

pub fn open_window(app: &AppHandle, date: Option<String>) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(PLANNER_LABEL) {
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
    } else {
        WebviewWindowBuilder::new(app, PLANNER_LABEL, WebviewUrl::App("index.html".into()))
            .title("Джарвис — планировщик")
            .inner_size(1280.0, 820.0)
            .min_inner_size(960.0, 620.0)
            .center()
            .theme(Some(tauri::Theme::Light))
            .background_color(tauri::webview::Color(255, 255, 255, 255))
            .decorations(false)
            .shadow(true)
            .build()?;
    }
    if let Some(d) = date.filter(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok()) {
        let _ = app.emit_to(PLANNER_LABEL, "planner:goto", d);
    }
    Ok(())
}

#[tauri::command]
pub fn planner_events(planner: State<'_, Arc<Planner>>, from: String, to: String) -> AppResult<Vec<CalEvent>> {
    planner.list(&from, &to)
}

#[tauri::command]
pub fn planner_create(planner: State<'_, Arc<Planner>>, input: EventInput) -> AppResult<Vec<CalEvent>> {
    planner.create(input)
}

#[tauri::command]
pub fn planner_update(app: AppHandle, planner: State<'_, Arc<Planner>>, id: String, patch: EventPatch) -> AppResult<CalEvent> {
    let done_now = patch.done == Some(true) && planner.get(&id).is_ok_and(|e| !e.done);
    let ev = planner.update(&id, patch)?;
    if done_now {
        // A finished block is worth XP: progress you can see keeps you going.
        if let Some(m) = app.try_state::<Arc<crate::mentor::Mentor>>() {
            let _ = m.log("block", &ev.category, &ev.title, 0.0, 15, 0, serde_json::json!({ "event": ev.id }));
            m.emit_state();
        }
    }
    Ok(ev)
}

#[tauri::command]
pub fn planner_delete(planner: State<'_, Arc<Planner>>, id: String, following: Option<bool>) -> AppResult<usize> {
    planner.delete(&id, following.unwrap_or(false))
}

#[tauri::command]
pub async fn planner_suggest(app: AppHandle, planner: State<'_, Arc<Planner>>, date: String, wishes: Option<String>) -> AppResult<Vec<Suggestion>> {
    let date = NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").map_err(|_| AppError::invalid("Дата ожидается как ГГГГ-ММ-ДД"))?;
    let planner = planner.inner().clone();
    suggest::suggest(&app, &planner, date, Local::now().naive_local(), wishes.as_deref()).await
}

#[tauri::command]
pub fn planner_google_status(planner: State<'_, Arc<Planner>>) -> SyncStatus {
    google::status(&planner)
}

#[tauri::command]
pub async fn planner_google_connect(planner: State<'_, Arc<Planner>>, client_id: String, client_secret: String) -> AppResult<SyncStatus> {
    let planner = planner.inner().clone();
    google::connect(&planner, client_id, client_secret).await
}

#[tauri::command]
pub async fn planner_google_disconnect(planner: State<'_, Arc<Planner>>) -> AppResult<SyncStatus> {
    let planner = planner.inner().clone();
    google::disconnect(&planner).await
}

#[tauri::command]
pub async fn planner_google_sync(planner: State<'_, Arc<Planner>>) -> AppResult<SyncStatus> {
    let planner = planner.inner().clone();
    google::sync(&planner).await
}
