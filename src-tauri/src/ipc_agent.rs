//! IPC for the agent, settings, secrets, memories and history.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::agent::{Agent, AgentStatus, HistoryItem, SendResult};
use crate::ai::{self, types::{ChatMessage, Role}};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::{secrets, settings::{Settings, ProviderPreset, PRESETS}, Memory, TaskRecord};

// ---------- agent ----------

#[tauri::command]
pub fn agent_status(agent: State<'_, Arc<Agent>>) -> AgentStatus {
    agent.status()
}

#[tauri::command]
pub fn agent_send(agent: State<'_, Arc<Agent>>, text: String, attachments: Vec<String>) -> AppResult<SendResult> {
    agent.send(text, attachments)
}

#[tauri::command]
pub fn agent_cancel(agent: State<'_, Arc<Agent>>) {
    agent.cancel();
}

#[tauri::command]
pub fn agent_confirm(agent: State<'_, Arc<Agent>>, call_id: String, approved: bool) -> AppResult<()> {
    agent.confirm(&call_id, approved)
}

#[tauri::command]
pub fn agent_new_conversation(agent: State<'_, Arc<Agent>>) -> AppResult<()> {
    agent.new_conversation()
}

#[tauri::command]
pub fn agent_history(agent: State<'_, Arc<Agent>>) -> Vec<HistoryItem> {
    agent.history()
}

// ---------- settings ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub presets: &'static [ProviderPreset],
    pub has_llm_key: bool,
    pub has_voice_key: bool,
    pub log_path: Option<String>,
    pub data_path: Option<String>,
}

#[tauri::command]
pub fn settings_get(app: AppHandle, state: State<'_, AppState>) -> SettingsView {
    let log_dir = dirs::data_local_dir().map(|d| d.join("dev.jarvis.desktop").join("logs").display().to_string());
    SettingsView {
        settings: state.store.settings(),
        presets: PRESETS,
        has_llm_key: secrets::exists(secrets::LLM_API_KEY),
        has_voice_key: secrets::exists(secrets::VOICE_API_KEY),
        log_path: log_dir,
        data_path: app.path().app_local_data_dir().ok().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub fn settings_save(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> AppResult<()> {
    let old = state.store.settings();
    state.store.save_settings(&settings)?;
    if old.launch_at_startup != settings.launch_at_startup {
        use tauri_plugin_autostart::ManagerExt;
        let al = app.autolaunch();
        let res = if settings.launch_at_startup { al.enable() } else { al.disable() };
        res.map_err(|e| AppError::platform(format!("Автозапуск: {e}")))?;
    }
    if old.shortcuts.workspace != settings.shortcuts.workspace || old.shortcuts.voice != settings.shortcuts.voice {
        crate::shortcuts::apply(&app, &settings.shortcuts);
    }
    let _ = app.emit("settings:changed", ());
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretKind {
    Llm,
    Voice,
}

fn target(kind: &SecretKind) -> &'static str {
    match kind {
        SecretKind::Llm => secrets::LLM_API_KEY,
        SecretKind::Voice => secrets::VOICE_API_KEY,
    }
}

/// `base_url`: the API address shown in Settings when the key is saved; the
/// LLM key is bound to its origin and never sent anywhere else.
#[tauri::command]
pub fn secret_set(app: AppHandle, state: State<'_, AppState>, kind: SecretKind, value: String, base_url: Option<String>) -> AppResult<()> {
    let v = value.trim();
    if v.is_empty() {
        return Err(AppError::invalid("Пустой ключ"));
    }
    if matches!(kind, SecretKind::Llm) {
        let url = base_url.filter(|u| !u.trim().is_empty()).unwrap_or_else(|| state.store.settings().provider.effective_base_url());
        if url.trim().is_empty() {
            return Err(AppError::invalid("Сначала укажите адрес API провайдера"));
        }
        state.store.kv_set(ai::KEY_ORIGIN, &ai::origin_of(&url))?;
    }
    secrets::set(target(&kind), v)?;
    let _ = app.emit("settings:changed", ());
    Ok(())
}

#[tauri::command]
pub fn secret_clear(app: AppHandle, state: State<'_, AppState>, kind: SecretKind) -> AppResult<()> {
    secrets::delete(target(&kind))?;
    if matches!(kind, SecretKind::Llm) {
        state.store.kv_set(ai::KEY_ORIGIN, "")?;
    }
    let _ = app.emit("settings:changed", ());
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTest {
    pub ok: bool,
    pub latency_ms: u64,
    pub model: String,
    pub reply: String,
}

/// Send a tiny request to verify endpoint, key and model.
#[tauri::command]
pub async fn provider_test(state: State<'_, AppState>) -> AppResult<ProviderTest> {
    let settings = state.store.settings();
    let client = ai::client_from_settings(&settings, &state.store)?;
    let started = Instant::now();
    let cancel = tokio_util::sync::CancellationToken::new();
    let msgs = vec![ChatMessage::text(Role::User, "Ответь одним словом: «готов».")];
    let c = client.complete(&msgs, &[], &cancel, Some(16), |_| {}).await?;
    Ok(ProviderTest {
        ok: true,
        latency_ms: started.elapsed().as_millis() as u64,
        model: client.model.clone(),
        reply: c.message.joined_text().chars().take(80).collect(),
    })
}

// ---------- memory & history ----------

#[tauri::command]
pub fn memories_list(state: State<'_, AppState>) -> AppResult<Vec<Memory>> {
    state.store.memories()
}

#[tauri::command]
pub fn memory_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.store.delete_memory(id)
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>, agent: State<'_, Arc<Agent>>) -> AppResult<()> {
    state.store.clear_history()?;
    let _ = agent.new_conversation();
    Ok(())
}

#[tauri::command]
pub fn tasks_recent(state: State<'_, AppState>) -> AppResult<Vec<TaskRecord>> {
    state.store.recent_tasks(50)
}

// ---------- windows ----------

#[tauri::command]
pub async fn settings_open(app: AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html".into()))
        .title("JARVIS — настройки")
        .inner_size(820.0, 640.0)
        .min_inner_size(680.0, 520.0)
        .center()
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::webview::Color(12, 12, 14, 255))
        .build()?;
    Ok(())
}
