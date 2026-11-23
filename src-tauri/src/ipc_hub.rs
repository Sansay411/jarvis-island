//! Tauri IPC commands for the Notch Hub (Tasks, Focus, Shelf, Activity Heatmap, Routines).

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::agent::{Agent, SendResult};
use crate::error::AppResult;
use crate::hub::{ActivityHeatmap, FocusState, Hub, RoutineItem, ShelfItem, TaskItem};
use crate::platform;
use crate::state::AppState;

// ---------- tasks ----------

#[tauri::command]
pub fn hub_tasks_list(hub: State<'_, Arc<Hub>>) -> AppResult<Vec<TaskItem>> {
    hub.tasks_list()
}

#[tauri::command]
pub fn hub_task_add(
    hub: State<'_, Arc<Hub>>,
    title: String,
    scheduled_date: Option<String>,
    scheduled_time: Option<String>,
    color: Option<String>,
    notes: Option<String>,
) -> AppResult<TaskItem> {
    hub.task_add_with_notes(title, notes.unwrap_or_default(), scheduled_date, scheduled_time, color)
}

#[tauri::command]
pub fn hub_task_toggle(hub: State<'_, Arc<Hub>>, id: String) -> AppResult<TaskItem> {
    hub.task_toggle(&id)
}

#[tauri::command]
pub fn hub_task_remove(hub: State<'_, Arc<Hub>>, id: String) -> AppResult<()> {
    hub.task_remove(&id)
}

// ---------- focus ----------

#[tauri::command]
pub fn hub_focus_get(hub: State<'_, Arc<Hub>>) -> AppResult<FocusState> {
    hub.focus_get()
}

#[tauri::command]
pub fn hub_focus_start(
    hub: State<'_, Arc<Hub>>,
    title: Option<String>,
    minutes: u32,
    task_id: Option<String>,
) -> AppResult<FocusState> {
    hub.focus_start(title.unwrap_or_default(), minutes, task_id)
}

#[tauri::command]
pub fn hub_focus_pause_toggle(hub: State<'_, Arc<Hub>>) -> AppResult<FocusState> {
    hub.focus_pause_toggle()
}

#[tauri::command]
pub fn hub_focus_complete(hub: State<'_, Arc<Hub>>) -> AppResult<FocusState> {
    hub.focus_complete()
}

#[tauri::command]
pub fn hub_focus_cancel(hub: State<'_, Arc<Hub>>) -> AppResult<FocusState> {
    hub.focus_cancel()
}

// ---------- shelf (tray) ----------

#[tauri::command]
pub fn hub_shelf_list(hub: State<'_, Arc<Hub>>) -> AppResult<Vec<ShelfItem>> {
    hub.shelf_list()
}

#[tauri::command]
pub async fn hub_shelf_add_files(hub: State<'_, Arc<Hub>>, paths: Vec<String>) -> AppResult<Vec<ShelfItem>> {
    let h = hub.inner().clone();
    platform::blocking(move || h.shelf_add_files(paths)).await
}

#[tauri::command]
pub fn hub_shelf_add_text(hub: State<'_, Arc<Hub>>, text: String) -> AppResult<ShelfItem> {
    hub.shelf_add_text(text)
}

#[tauri::command]
pub fn hub_shelf_remove(hub: State<'_, Arc<Hub>>, id: String) -> AppResult<()> {
    hub.shelf_remove(&id)
}

/// Thumbnail for a shelf tile (data URL), loaded lazily by the tile.
#[tauri::command]
pub async fn hub_shelf_thumbnail(path: String) -> AppResult<Option<String>> {
    platform::blocking(move || Ok(crate::filekit::shelf_thumbnail(std::path::Path::new(&path)))).await
}

/// Drags shelf files out of the island (native shell drag on the UI thread).
/// Resolves when the drop happened or was cancelled; `true` = accepted.
#[tauri::command]
pub async fn hub_shelf_drag_out(window: tauri::WebviewWindow, paths: Vec<String>) -> AppResult<bool> {
    platform::dragout::validate_drag_paths(&paths)?;
    #[cfg(windows)]
    let hwnd = window.hwnd().map_err(|e| crate::error::AppError::platform(e.to_string()))?.0 as isize;
    #[cfg(target_os = "macos")]
    let hwnd = 0isize;
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .run_on_main_thread(move || {
            let hwnd = crate::platform::HWND(hwnd as _);
            let _ = tx.send(platform::dragout::drag_files_out(hwnd, &paths));
        })
        .map_err(|e| crate::error::AppError::platform(e.to_string()))?;
    rx.await.map_err(|_| crate::error::AppError::internal("Перетаскивание не завершилось"))?
}

#[tauri::command]
pub fn hub_shelf_clear(hub: State<'_, Arc<Hub>>) -> AppResult<()> {
    hub.shelf_clear()
}

// ---------- activity heatmap ----------

#[tauri::command]
pub async fn hub_activity_get(hub: State<'_, Arc<Hub>>) -> AppResult<ActivityHeatmap> {
    let h = hub.inner().clone();
    platform::blocking(move || h.activity_get()).await
}

// ---------- GitHub contribution calendar («Стрик») ----------

#[tauri::command]
pub async fn hub_github_get(state: State<'_, AppState>, force: Option<bool>) -> AppResult<crate::github::GithubState> {
    let store = state.store.clone();
    platform::blocking(move || crate::github::state(&store, force.unwrap_or(false))).await
}

#[tauri::command]
pub async fn hub_github_set_login(state: State<'_, AppState>, login: String) -> AppResult<crate::github::GithubState> {
    let store = state.store.clone();
    platform::blocking(move || crate::github::set_login(&store, &login)).await
}

// ---------- weather (island card and overview) ----------

#[tauri::command]
pub async fn weather_get(place: Option<String>) -> AppResult<crate::geo::Weather> {
    platform::blocking(move || crate::geo::weather(place.as_deref())).await
}

// ---------- routines ----------

#[tauri::command]
pub fn hub_routines_list(hub: State<'_, Arc<Hub>>) -> AppResult<Vec<RoutineItem>> {
    hub.routines_list()
}

#[tauri::command]
pub fn hub_routine_save(
    hub: State<'_, Arc<Hub>>,
    id: Option<String>,
    title: String,
    subtitle: Option<String>,
    icon: Option<String>,
    prompt: String,
    autonomy: Option<bool>,
) -> AppResult<RoutineItem> {
    hub.routine_save(
        id,
        title,
        subtitle.unwrap_or_default(),
        icon.unwrap_or_else(|| "zap".into()),
        prompt,
        autonomy.unwrap_or(false),
    )
}

#[tauri::command]
pub fn hub_routine_remove(hub: State<'_, Arc<Hub>>, id: String) -> AppResult<()> {
    hub.routine_remove(&id)
}

#[tauri::command]
pub fn hub_routine_run(
    app: AppHandle,
    state: State<'_, AppState>,
    hub: State<'_, Arc<Hub>>,
    agent: State<'_, Arc<Agent>>,
    id: String,
) -> AppResult<SendResult> {
    let routine = hub.routine_get(&id)?;
    state.island.set_user_hidden(false);
    crate::shortcuts::emit_command(&app, "open-workspace");
    if routine.autonomy {
        agent.send_routine(routine.prompt, true)
    } else {
        agent.send_external(routine.prompt)
    }
}
