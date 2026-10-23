//! Tauri commands for files dropped on the island (see `filekit`).

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::filekit::{self, FileJobs, Inspection};

#[tauri::command]
pub async fn files_inspect(paths: Vec<String>) -> AppResult<Inspection> {
    *filekit::SHOWN.lock() = paths.clone();
    crate::platform::blocking(move || filekit::inspect(&paths)).await
}

/// Start an action; progress and the verified result arrive as `files:job`.
#[tauri::command]
pub fn files_action(app: AppHandle, jobs: State<'_, Arc<FileJobs>>, paths: Vec<String>, action: String) -> String {
    tracing::info!(action = %action, files = paths.len(), "file action requested");
    jobs.start(app, paths, action)
}

/// The card was closed: «этот файл» no longer points anywhere.
#[tauri::command]
pub fn files_clear() {
    filekit::SHOWN.lock().clear();
}

#[tauri::command]
pub fn files_cancel(jobs: State<'_, Arc<FileJobs>>, job_id: String) -> bool {
    jobs.cancel(&job_id)
}

#[tauri::command]
pub fn reminders_list(reminders: State<'_, Arc<crate::reminders::Reminders>>) -> Vec<crate::store::Reminder> {
    reminders.list()
}

/// «Готово» on an alert, or cancel a pending one.
#[tauri::command]
pub fn reminder_done(reminders: State<'_, Arc<crate::reminders::Reminders>>, id: i64) -> AppResult<()> {
    reminders.done(id)
}

/// «Позже»: fire again in `minutes`.
#[tauri::command]
pub fn reminder_snooze(reminders: State<'_, Arc<crate::reminders::Reminders>>, id: i64, minutes: u32) -> AppResult<()> {
    reminders.snooze(id, minutes)
}
