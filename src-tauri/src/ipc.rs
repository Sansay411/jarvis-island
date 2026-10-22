//! Tauri IPC commands. Commands are thin: they validate input, run the
//! blocking platform call off the async runtime and return typed results.
//! Business logic lives in `platform`, `tools` and `agent`.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::platform::{self, apps, audio, brightness, display, island::ShapeRect, media, winmgr};
use crate::state::AppState;

// ---------- island ----------

#[tauri::command]
pub fn island_set_shape(state: State<'_, AppState>, shape: ShapeRect) {
    state.island.set_shape(shape);
}

#[tauri::command]
pub fn island_begin_input(state: State<'_, AppState>) -> AppResult<()> {
    state.island.begin_input()
}

#[tauri::command]
pub fn island_end_input(state: State<'_, AppState>, restore: bool) -> AppResult<()> {
    state.island.end_input(restore)
}

#[tauri::command]
pub fn island_set_hidden(state: State<'_, AppState>, hidden: bool) {
    state.island.set_user_hidden(hidden);
}

// ---------- audio ----------

#[tauri::command]
pub async fn audio_get() -> AppResult<audio::AudioState> {
    platform::blocking(audio::get_state).await
}

#[tauri::command]
pub async fn audio_set_volume(percent: u8) -> AppResult<audio::AudioState> {
    platform::blocking(move || audio::set_volume(percent)).await
}

#[tauri::command]
pub async fn audio_set_mute(muted: bool) -> AppResult<audio::AudioState> {
    platform::blocking(move || audio::set_mute(muted)).await
}

// ---------- brightness ----------

#[tauri::command]
pub async fn brightness_list() -> AppResult<Vec<brightness::DisplayBrightness>> {
    platform::blocking(brightness::list).await
}

#[tauri::command]
pub async fn brightness_set(percent: u8, target: Option<String>) -> AppResult<Vec<brightness::DisplayBrightness>> {
    platform::blocking(move || brightness::set(percent, target.as_deref())).await
}

// ---------- media ----------

#[tauri::command]
pub async fn media_get() -> AppResult<media::MediaState> {
    platform::blocking(media::snapshot).await
}

#[tauri::command]
pub async fn media_artwork() -> AppResult<media::ArtworkEvent> {
    platform::blocking(media::artwork).await
}

#[tauri::command]
pub async fn media_control(action: media::MediaAction) -> AppResult<media::ControlOutcome> {
    platform::blocking(move || media::control(action)).await
}

// ---------- windows & apps ----------

#[tauri::command]
pub async fn windows_list() -> AppResult<Vec<winmgr::WindowInfo>> {
    platform::blocking(|| Ok(winmgr::list_windows())).await
}

#[tauri::command]
pub async fn window_focus(hwnd: i64) -> AppResult<winmgr::WindowInfo> {
    platform::blocking(move || winmgr::focus(hwnd)).await
}

#[tauri::command]
pub async fn apps_search(state: State<'_, AppState>, query: String) -> AppResult<Vec<apps::AppEntry>> {
    let index = state.apps.clone();
    platform::blocking(move || index.search(&query, 8)).await
}

#[tauri::command]
pub async fn app_launch(state: State<'_, AppState>, id: String) -> AppResult<apps::LaunchOutcome> {
    let index = state.apps.clone();
    platform::blocking(move || {
        let app = index
            .by_id(&id)?
            .ok_or_else(|| crate::error::AppError::not_found("Приложение не найдено"))?;
        apps::launch(&app, Duration::from_secs(12))
    })
    .await
}

#[tauri::command]
pub fn shortcut_problems() -> Vec<String> {
    crate::shortcuts::problems()
}

// ---------- diagnostics ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub uptime_secs: u64,
    pub tracker_hz: f64,
    pub monitors: Vec<display::MonitorInfo>,
    pub working_set_mb: f64,
}

#[tauri::command]
pub async fn diagnostics_get(state: State<'_, AppState>) -> AppResult<Diagnostics> {
    // Sample the pointer tracker rate over half a second.
    let ticks = state.island.tracker_ticks.load(std::sync::atomic::Ordering::Relaxed);
    let t0 = Instant::now();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let tracker_hz = state.island.tracker_rate(t0, ticks);
    Ok(Diagnostics {
        uptime_secs: state.started_at.elapsed().as_secs(),
        tracker_hz,
        monitors: display::list_monitors(),
        working_set_mb: crate::util::working_set_mb(),
    })
}

// ---------- background jobs ----------

#[tauri::command]
pub fn jobs_list(jobs: State<'_, std::sync::Arc<crate::jobs::JobManager>>) -> Vec<crate::jobs::JobView> {
    jobs.list()
}

#[tauri::command]
pub fn job_cancel(jobs: State<'_, std::sync::Arc<crate::jobs::JobManager>>, id: String) -> AppResult<()> {
    jobs.cancel(&id)
}

// ---------- voice ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub state: crate::voice::VoiceState,
    pub metrics: crate::voice::VoiceMetrics,
}

#[tauri::command]
pub fn voice_toggle(voice: State<'_, std::sync::Arc<crate::voice::VoiceManager>>) -> AppResult<bool> {
    if voice.is_active() {
        voice.stop();
        Ok(false)
    } else {
        voice.start()?;
        Ok(true)
    }
}

#[tauri::command]
pub fn voice_stop(voice: State<'_, std::sync::Arc<crate::voice::VoiceManager>>) {
    voice.stop();
}

#[tauri::command]
pub fn voice_status(voice: State<'_, std::sync::Arc<crate::voice::VoiceManager>>) -> VoiceStatus {
    VoiceStatus { state: voice.state(), metrics: voice.metrics.lock().clone() }
}

#[tauri::command]
pub async fn audio_devices() -> AppResult<crate::voice::audio::AudioDevices> {
    tokio::task::spawn_blocking(crate::voice::audio::list_devices)
        .await
        .map_err(|e| crate::error::AppError::internal(e.to_string()))
}

#[tauri::command]
pub fn stt_models() -> Vec<crate::voice::stt::ModelInfo> {
    crate::voice::stt::list()
}

#[tauri::command]
pub fn stt_download(voice: State<'_, std::sync::Arc<crate::voice::VoiceManager>>, id: String) -> AppResult<()> {
    voice.download_model(&id)
}

#[tauri::command]
pub fn stt_download_cancel(voice: State<'_, std::sync::Arc<crate::voice::VoiceManager>>) {
    voice.cancel_download();
}

#[tauri::command]
pub async fn tts_voices() -> AppResult<Vec<crate::voice::tts::TtsVoice>> {
    platform::blocking(crate::voice::tts::voices).await
}

#[tauri::command]
pub fn island_context(state: State<'_, AppState>) -> serde_json::Value {
    serde_json::json!({ "topCovered": state.island.top_covered() })
}

/// Frame pacing measured by the island during shape transitions.
#[tauri::command]
pub fn perf_report(label: String, fps: u32, worst_ms: u32, frames: u32) {
    tracing::info!(label, fps, worst_ms, frames, "island animation");
}
