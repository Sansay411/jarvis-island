//! JARVIS — native AI desktop agent for Windows 11.
//!
//! Layering (see docs/ARCHITECTURE.md):
//!   platform  → Win32/COM/WinRT integrations, no UI knowledge
//!   store     → SQLite settings/history/audit, Credential Manager secrets
//!   ai        → provider adapters (OpenAI-compatible)
//!   tools     → registry of verified agent capabilities
//!   agent     → the Observe → Reason → Act → Verify loop
//!   ipc*      → Tauri commands/events, thin adapters
//!   state     → shared handles managed by Tauri

pub mod agent;
pub mod ai;
pub mod error;
mod ipc;
pub mod jobs;
mod ipc_agent;
pub mod platform;
mod shortcuts;
mod state;
pub mod store;
pub mod tools;
pub mod util;
pub mod voice;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

use platform::island::{self, Island};
use shortcuts::emit_command;
use state::AppState;

fn init_logging() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,jarvis_lib=debug"));
    let log_dir = dirs::data_local_dir().map(|d| d.join("dev.jarvis.desktop").join("logs"));
    let file = log_dir.and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::OpenOptions::new().create(true).append(true).open(dir.join("jarvis.log")).ok()
    });
    match file {
        Some(f) => fmt().with_env_filter(filter).with_ansi(false).with_writer(std::sync::Mutex::new(f)).init(),
        None => fmt().with_env_filter(filter).init(),
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let chat = MenuItem::with_id(app, "chat", "Открыть чат", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Показать или скрыть остров", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Настройки", true, None::<&str>)?;
    let cancel = MenuItem::with_id(app, "cancel", "Остановить текущую задачу", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти из JARVIS", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&chat, &toggle, &sep, &cancel, &settings, &sep2, &quit])?;
    TrayIconBuilder::with_id("jarvis")
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .tooltip("JARVIS")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => app.state::<AppState>().island.toggle_user_hidden(),
            "chat" => {
                app.state::<AppState>().island.set_user_hidden(false);
                emit_command(app, "open-workspace");
            }
            "cancel" => app.state::<Arc<agent::Agent>>().cancel(),
            "settings" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = ipc_agent::settings_open(app).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                app.state::<AppState>().island.set_user_hidden(false);
                emit_command(app, "open-workspace");
            }
        })
        .build(app)?;
    Ok(())
}

/// Handle `--ask TEXT [--quiet]`, `--command NAME` and `--settings`.
/// Returns false when argv carries none of them.
fn handle_cli(app: &tauri::AppHandle, argv: &[String]) -> bool {
    if let Some(pos) = argv.iter().position(|a| a == "--ask") {
        if let Some(text) = argv.get(pos + 1).cloned() {
            // --quiet runs the task without opening the chat (scripts, tests).
            if !argv.iter().any(|a| a == "--quiet") {
                app.state::<AppState>().island.set_user_hidden(false);
                emit_command(app, "open-workspace");
            }
            let agent = app.state::<Arc<agent::Agent>>();
            if let Err(e) = agent.send(text, Vec::new()) {
                tracing::warn!("--ask rejected: {e}");
                agent.post_report(format!("Не удалось начать задачу: {e}"), false);
            }
        }
        return true;
    }
    // `--command toggle-workspace|open-workspace|toggle-voice` lets
    // launchers, Stream Deck buttons and scripts drive the island.
    if let Some(pos) = argv.iter().position(|a| a == "--command") {
        if let Some(cmd) = argv.get(pos + 1) {
            if matches!(cmd.as_str(), "toggle-workspace" | "open-workspace" | "toggle-voice") {
                app.state::<AppState>().island.set_user_hidden(false);
                emit_command(app, cmd);
            }
        }
        return true;
    }
    if argv.iter().any(|a| a == "--settings") {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = ipc_agent::settings_open(app).await;
        });
        return true;
    }
    false
}

pub fn run() {
    init_logging();
    tracing::info!("JARVIS {} starting", env!("CARGO_PKG_VERSION"));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // A second launch (Start menu, shortcut, `jarvis.exe --settings`)
            // surfaces the running instance instead of starting another.
            if !handle_cli(app, &argv) {
                app.state::<AppState>().island.set_user_hidden(false);
                emit_command(app, "open-workspace");
            }
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(shortcuts::plugin())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            let store = Arc::new(store::Store::open(&data_dir.join("jarvis.db"))?);
            let settings = store.settings();

            let window = app
                .get_webview_window(island::LABEL)
                .ok_or("island window missing from configuration")?;
            let island = Island::new(app.handle().clone(), window.clone())?;
            let history = Arc::new(platform::winmgr::ForegroundHistory::default());
            let apps = Arc::new(platform::apps::AppIndex::default());

            app.manage(AppState {
                island: island.clone(),
                history: history.clone(),
                apps: apps.clone(),
                store,
                started_at: Instant::now(),
            });
            let registry = Arc::new(tools::Registry::new());
            app.manage(agent::Agent::new(app.handle().clone(), registry));
            app.manage(jobs::JobManager::new(app.handle().clone()));
            app.manage(voice::VoiceManager::new(app.handle().clone()));

            window.show()?;
            island.spawn_tracker();
            platform::watcher::spawn(island.clone(), history);
            if let Err(e) = platform::media::MediaWatcher::spawn(app.handle().clone()) {
                tracing::warn!("media watcher not started: {e}");
            }
            // Warm the app index in the background so the first "open X"
            // doesn't pay for Start-menu enumeration.
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let _ = platform::blocking(move || apps.ensure_fresh(Duration::from_secs(600))).await;
            });

            setup_tray(app)?;
            // First launch with CLI arguments: act once the island UI is listening.
            let argv: Vec<String> = std::env::args().collect();
            if argv.len() > 1 {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    handle_cli(&handle, &argv);
                });
            }
            for p in shortcuts::apply(app.handle(), &settings.shortcuts) {
                tracing::warn!("{p}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != island::LABEL {
                return;
            }
            match event {
                WindowEvent::Focused(false) => window.state::<AppState>().island.on_focus_lost(),
                WindowEvent::ScaleFactorChanged { .. } => {
                    let _ = window.state::<AppState>().island.place();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            ipc::island_set_shape,
            ipc::island_begin_input,
            ipc::island_end_input,
            ipc::island_set_hidden,
            ipc::island_context,
            ipc::audio_get,
            ipc::audio_set_volume,
            ipc::audio_set_mute,
            ipc::brightness_list,
            ipc::brightness_set,
            ipc::media_get,
            ipc::media_artwork,
            ipc::media_control,
            ipc::windows_list,
            ipc::window_focus,
            ipc::apps_search,
            ipc::app_launch,
            ipc::diagnostics_get,
            ipc::shortcut_problems,
            ipc::jobs_list,
            ipc::voice_toggle,
            ipc::voice_stop,
            ipc::voice_status,
            ipc::audio_devices,
            ipc::job_cancel,
            ipc_agent::agent_status,
            ipc_agent::agent_send,
            ipc_agent::agent_cancel,
            ipc_agent::agent_confirm,
            ipc_agent::agent_new_conversation,
            ipc_agent::agent_history,
            ipc_agent::settings_get,
            ipc_agent::settings_save,
            ipc_agent::secret_set,
            ipc_agent::secret_clear,
            ipc_agent::provider_test,
            ipc_agent::memories_list,
            ipc_agent::memory_delete,
            ipc_agent::history_clear,
            ipc_agent::tasks_recent,
            ipc_agent::settings_open,
        ])
        .run(tauri::generate_context!())
        .expect("error while running JARVIS");
}
