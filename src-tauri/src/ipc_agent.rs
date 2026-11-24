//! IPC for the agent, settings, secrets, memories and history.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::agent::{Agent, AgentStatus, HistoryItem, SendResult};
use crate::ai::{self, types::{ChatMessage, Role}};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::{
    secrets,
    settings::{should_auto_open_onboarding, ProviderPreset, Settings, ONBOARDING_VERSION, PRESETS},
    Memory, TaskRecord,
};

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

#[tauri::command]
pub fn agent_set_full_control(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> AppResult<bool> {
    let mut settings = state.store.settings();
    settings.agent.full_control = enabled;
    state.store.save_settings(&settings)?;
    let _ = app.emit("settings:changed", ());
    Ok(enabled)
}

// ---------- settings ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub presets: &'static [ProviderPreset],
    pub has_llm_key: bool,
    pub has_voice_key: bool,
    pub has_azure_key: bool,
    pub has_elevenlabs_key: bool,
    pub has_yandex_key: bool,
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
        has_azure_key: crate::voice::azure::configured(),
        has_elevenlabs_key: crate::voice::elevenlabs::configured(),
        has_yandex_key: crate::voice::yandex::key().is_some(),
        log_path: log_dir,
        data_path: app.path().app_local_data_dir().ok().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub fn settings_save(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> AppResult<()> {
    let old = state.store.settings();
    state.store.save_settings(&settings)?;
    if old.launch_at_startup != settings.launch_at_startup {
        crate::sync_autostart(&app, settings.launch_at_startup)
            .map_err(|e| AppError::platform(format!("Автозапуск: {e}")))?;
    }
    if old.shortcuts.workspace != settings.shortcuts.workspace
        || old.shortcuts.voice != settings.shortcuts.voice
        || old.shortcuts.clipboard != settings.shortcuts.clipboard
    {
        crate::shortcuts::apply(&app, &settings.shortcuts);
    }
    let _ = app.emit("settings:changed", ());
    if old.voice.wake_word != settings.voice.wake_word || old.voice.enabled != settings.voice.enabled || old.voice.engine != settings.voice.engine {
        app.state::<Arc<crate::voice::VoiceManager>>().apply_settings();
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretKind {
    Llm,
    Voice,
    /// Azure Speech key; `base_url` carries its region.
    Azure,
    Elevenlabs,
    Yandex,
}

fn target(kind: &SecretKind) -> &'static str {
    match kind {
        SecretKind::Llm => secrets::LLM_API_KEY,
        SecretKind::Voice => secrets::VOICE_API_KEY,
        SecretKind::Azure => secrets::AZURE_SPEECH_KEY,
        SecretKind::Elevenlabs => secrets::ELEVENLABS_KEY,
        SecretKind::Yandex => secrets::YANDEX_SPEECH_KEY,
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
    if matches!(kind, SecretKind::Azure) {
        let region = base_url.as_deref().unwrap_or("").trim().to_lowercase();
        if region.is_empty() || !region.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(AppError::invalid("Укажите регион ресурса Azure, например westeurope"));
        }
        secrets::set(secrets::AZURE_SPEECH_REGION, &region)?;
    }
    if matches!(kind, SecretKind::Llm) {
        let url = base_url.filter(|u| !u.trim().is_empty()).unwrap_or_else(|| state.store.settings().provider.effective_base_url());
        if url.trim().is_empty() {
            return Err(AppError::invalid("Сначала укажите адрес API провайдера"));
        }
        state.store.kv_set(ai::KEY_ORIGIN, &ai::origin_of(&url))?;
    }
    secrets::set(target(&kind), v)?;
    if matches!(kind, SecretKind::Yandex) {
        crate::voice::yandex::forget_refusal();
    }
    let _ = app.emit("settings:changed", ());
    Ok(())
}

#[tauri::command]
pub fn secret_clear(app: AppHandle, state: State<'_, AppState>, kind: SecretKind) -> AppResult<()> {
    secrets::delete(target(&kind))?;
    if matches!(kind, SecretKind::Azure) {
        let _ = secrets::delete(secrets::AZURE_SPEECH_REGION);
    }
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

fn effective_probe_url(preset: &str, base_url: &str) -> String {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        crate::store::settings::preset(preset).base_url.to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

/// Probe a local or custom model for vision, tool-calling, and context window.
#[tauri::command]
pub async fn model_probe(
    state: State<'_, AppState>,
    agent: State<'_, Arc<Agent>>,
    preset: String,
    base_url: String,
    model: String,
    active: Option<bool>,
    refresh: Option<bool>,
) -> AppResult<ai::probe::ModelProbe> {
    let url = effective_probe_url(&preset, &base_url);
    ai::probe::probe_model(&state, agent.registry(), &preset, &url, &model, active.unwrap_or(false), refresh.unwrap_or(false)).await
}

/// List models exposed by a local or custom OpenAI-compatible server.
#[tauri::command]
pub async fn models_list(
    state: State<'_, AppState>,
    agent: State<'_, Arc<Agent>>,
    preset: String,
    base_url: String,
) -> AppResult<ai::probe::ModelsListResult> {
    let url = effective_probe_url(&preset, &base_url);
    ai::probe::list_models(&state, agent.registry(), &preset, &url).await
}

// ---------- memory & history ----------

fn norm_memory(t: &str) -> String {
    t.to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[tauri::command]
pub fn memories_list(state: State<'_, AppState>) -> AppResult<Vec<Memory>> {
    state.store.memories()
}

#[tauri::command]
pub fn memory_add(state: State<'_, AppState>, text: String) -> AppResult<Memory> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("Пустая заметка"));
    }
    let key = norm_memory(trimmed);
    let existing = state.store.memories()?;
    if let Some(known) = existing.into_iter().find(|m| norm_memory(&m.text) == key) {
        return Ok(known);
    }
    let id = state.store.add_memory(trimmed)?;
    state
        .store
        .memories()?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::internal("Не удалось сохранить заметку"))
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
        .inner_size(960.0, 700.0)
        .min_inner_size(780.0, 560.0)
        .center()
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::webview::Color(8, 8, 10, 255))
        // Our own title band (TitleBar.tsx) in the page's colour; the
        // shadow keeps Windows' rounded corners and resize edges.
        .decorations(false)
        .shadow(true)
        .build()?;
    Ok(())
}

// ---------- welcome (onboarding) ----------

pub const WELCOME_LABEL: &str = "welcome";

static WELCOME_AUTO_CHECKED: AtomicBool = AtomicBool::new(false);
static WELCOME_HANDLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WelcomeGreeting {
    pub title: String,
    pub sub: String,
    pub chat_line: String,
}

pub(crate) fn welcome_greeting(user_name: &str, address: &str, shortcut: &str) -> WelcomeGreeting {
    let name = user_name.trim();
    let vy = address == "vy";
    let key = if shortcut.trim().is_empty() { "Ctrl+Alt+J" } else { shortcut.trim() };
    let title = if name.is_empty() {
        "Приятно познакомиться!".to_string()
    } else if vy {
        format!("Рад знакомству, {name}!")
    } else {
        format!("Привет, {name}!")
    };
    let sub = format!("Я всё запомнил · чат по {key}");
    let chat_line = if name.is_empty() {
        if vy {
            format!("Приятно познакомиться! Я готов к работе — нажмите {key} или просто позовите меня, когда понадоблюсь.")
        } else {
            format!("Приятно познакомиться! Я готов к работе — нажми {key} или просто позови меня, когда понадоблюсь.")
        }
    } else if vy {
        format!("Рад знакомству, {name}! Я всё запомнил — нажмите {key} или просто позовите меня, когда понадоблюсь.")
    } else {
        format!("Привет, {name}! Я всё запомнил — нажми {key} или просто позови меня, когда понадоблюсь.")
    };
    WelcomeGreeting { title, sub, chat_line }
}

pub fn open_welcome_window(app: &AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(WELCOME_LABEL) {
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    WELCOME_HANDLED.store(false, Ordering::Release);
    WebviewWindowBuilder::new(app, WELCOME_LABEL, WebviewUrl::App("index.html".into()))
        .title("JARVIS — знакомство")
        .inner_size(640.0, 560.0)
        .min_inner_size(580.0, 520.0)
        .center()
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::webview::Color(8, 8, 10, 255))
        .decorations(false)
        .shadow(true)
        .build()?;
    Ok(())
}

/// Open the first-run «Знакомство» window once per launch if the user has not
/// completed it, dismissed it fewer than 3 times, and has no existing name or
/// saved memories.
pub fn maybe_open_welcome(app: &AppHandle) {
    if WELCOME_AUTO_CHECKED.swap(true, Ordering::AcqRel) {
        return;
    }
    let state = app.state::<AppState>();
    let settings = state.store.settings();
    let memories_count = state.store.memories().map(|m| m.len()).unwrap_or(0);
    if !should_auto_open_onboarding(&settings, memories_count) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(650)).await;
        if let Err(e) = open_welcome_window(&handle) {
            tracing::warn!("welcome window not opened: {e}");
        }
    });
}

fn record_welcome_dismiss(app: &AppHandle, store: &crate::store::Store) {
    if WELCOME_HANDLED.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut settings = store.settings();
    if settings.onboarding.done {
        return;
    }
    settings.onboarding.dismissed_count = settings.onboarding.dismissed_count.saturating_add(1);
    if let Err(e) = store.save_settings(&settings) {
        tracing::warn!("onboarding dismiss not saved: {e}");
    } else {
        let _ = app.emit("settings:changed", ());
    }
}

pub fn on_welcome_destroyed(app: &AppHandle) {
    record_welcome_dismiss(app, &app.state::<AppState>().store);
}

#[tauri::command]
pub async fn welcome_open(app: AppHandle) -> AppResult<()> {
    open_welcome_window(&app)
}

#[tauri::command]
pub fn welcome_dismiss(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    record_welcome_dismiss(&app, &state.store);
    if let Some(w) = app.get_webview_window(WELCOME_LABEL) {
        let _ = w.close();
    }
    Ok(())
}

#[tauri::command]
pub fn welcome_complete(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    WELCOME_HANDLED.store(true, Ordering::Release);
    let mut settings = state.store.settings();
    settings.onboarding.done = true;
    settings.onboarding.version = ONBOARDING_VERSION;
    state.store.save_settings(&settings)?;
    let _ = app.emit("settings:changed", ());

    let greeting = welcome_greeting(&settings.persona.user_name, &settings.persona.address, &settings.shortcuts.workspace);
    let _ = app.emit_to(crate::platform::island::LABEL, "island:emote", "heart");
    let _ = app.emit_to(
        crate::platform::island::LABEL,
        "island:greet",
        serde_json::json!({ "title": greeting.title, "sub": greeting.sub }),
    );
    app.state::<Arc<Agent>>().post_report(greeting.chat_line, true);

    if let Some(w) = app.get_webview_window(WELCOME_LABEL) {
        let _ = w.close();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_greeting_uses_name_and_address() {
        let ty = welcome_greeting("Алексей", "ty", "Ctrl+Alt+J");
        assert_eq!(ty.title, "Привет, Алексей!");
        assert!(ty.chat_line.contains("нажми Ctrl+Alt+J"));

        let vy = welcome_greeting("Мария", "vy", "Alt+Space");
        assert_eq!(vy.title, "Рад знакомству, Мария!");
        assert!(vy.chat_line.contains("нажмите Alt+Space"));

        let anon = welcome_greeting("  ", "ty", "");
        assert_eq!(anon.title, "Приятно познакомиться!");
        assert!(anon.sub.contains("Ctrl+Alt+J"));
    }

    #[test]
    fn memory_normalization_deduplicates_yo_and_punctuation() {
        assert_eq!(norm_memory("Пользователь — студент, изучает Rust!"), norm_memory("пользователь студент изучает rust"));
        assert_eq!(norm_memory("Учёба и режим"), norm_memory("учеба и режим"));
    }
}
