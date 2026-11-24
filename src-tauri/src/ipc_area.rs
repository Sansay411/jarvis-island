//! IPC for «Спроси про область»: picker lifecycle, confirmation and pending
//! area attachment state.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::area::{self, AreaManager, PendingAreaPreview, PickerState};
use crate::error::AppResult;

#[tauri::command]
pub fn area_open_picker(app: AppHandle) -> AppResult<()> {
    area::open_picker(&app)
}

#[tauri::command]
pub fn area_picker_state(manager: State<'_, Arc<AreaManager>>) -> Option<PickerState> {
    manager.picker_state()
}

#[tauri::command]
pub async fn area_confirm_rect(app: AppHandle, x1: f64, y1: f64, x2: f64, y2: f64) -> AppResult<PendingAreaPreview> {
    area::confirm_rect(app, x1, y1, x2, y2).await
}

#[tauri::command]
pub async fn area_confirm_click(
    app: AppHandle,
    x: f64,
    y: f64,
    hwnd: Option<i64>,
) -> AppResult<PendingAreaPreview> {
    area::confirm_click(app, x, y, hwnd).await
}

#[tauri::command]
pub fn area_cancel(app: AppHandle) {
    area::cancel_picker(&app);
}

#[tauri::command]
pub fn area_pending(manager: State<'_, Arc<AreaManager>>) -> Option<PendingAreaPreview> {
    manager.pending_preview()
}

#[tauri::command]
pub fn area_clear(manager: State<'_, Arc<AreaManager>>) {
    manager.clear_pending();
}
