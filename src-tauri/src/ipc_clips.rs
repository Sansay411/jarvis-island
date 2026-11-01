//! Tauri commands for the clipboard history (see `clips`).

use std::sync::Arc;

use tauri::State;

use crate::clips::{Clips, Status};
use crate::error::AppResult;
use crate::store::clips::Clip;

#[tauri::command]
pub fn clips_list(clips: State<'_, Arc<Clips>>, query: Option<String>, limit: Option<u32>) -> AppResult<Vec<Clip>> {
    clips.list(query.as_deref().unwrap_or(""), limit.unwrap_or(60))
}

#[tauri::command]
pub fn clips_status(clips: State<'_, Arc<Clips>>) -> Status {
    clips.status()
}

/// «Пауза» in the panel: the clipboard stops being read at all.
#[tauri::command]
pub fn clips_set_enabled(clips: State<'_, Arc<Clips>>, enabled: bool) -> AppResult<()> {
    clips.set_enabled(enabled)
}

/// Put the entry on the clipboard and paste it into the window that had the
/// keyboard; returns the program's name.
#[tauri::command]
pub async fn clips_paste(clips: State<'_, Arc<Clips>>, id: i64) -> AppResult<String> {
    let clips = Arc::clone(clips.inner());
    clips.paste(id).await
}

/// Put the entry on the clipboard without pasting.
#[tauri::command]
pub async fn clips_copy(clips: State<'_, Arc<Clips>>, id: i64) -> AppResult<()> {
    let clips = Arc::clone(clips.inner());
    crate::platform::blocking(move || clips.copy(id).map(|_| ())).await
}

#[tauri::command]
pub fn clips_pin(clips: State<'_, Arc<Clips>>, id: i64, pinned: bool) -> AppResult<()> {
    clips.set_pinned(id, pinned)
}

#[tauri::command]
pub fn clips_delete(clips: State<'_, Arc<Clips>>, id: i64) -> AppResult<()> {
    clips.delete(id)
}

/// Returns how many entries were removed.
#[tauri::command]
pub fn clips_clear(clips: State<'_, Arc<Clips>>, keep_pinned: bool) -> AppResult<u32> {
    clips.clear(keep_pinned)
}
