//! JARVIS — native AI desktop agent for Windows 11.
//!
//! Layering (see docs/ARCHITECTURE.md):
//!   platform  → Win32/COM/WinRT integrations, no UI knowledge
//!   ipc       → Tauri commands/events, thin adapters over platform
//!   state     → shared handles managed by Tauri

pub mod error;
mod ipc;
pub mod platform;
mod state;
pub mod util;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use platform::island::{self, Island};
use state::AppState;

pub const EVT_COMMAND: &str = "island:command";

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

fn emit_command(app: &tauri::AppHandle, action: &str) {
    let _ = app.emit_to(island::LABEL, EVT_COMMAND, action);
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Показать или скрыть остров", true, None::<&str>)?;
    let chat = MenuItem::with_id(app, "chat", "Открыть чат", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти из JARVIS", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&chat, &toggle, &PredefinedMenuItem::separator(app)?, &quit])?;
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
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                let app = tray.app_handle();
                app.state::<AppState>().island.set_user_hidden(false);
                emit_command(app, "open-workspace");
            }
        })
        .build(app)?;
    Ok(())
}

fn setup_shortcuts(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyJ);
    let voice = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                app.state::<AppState>().island.set_user_hidden(false);
                if shortcut == &workspace {
                    emit_command(app, "toggle-workspace");
                } else if shortcut == &voice {
                    emit_command(app, "toggle-voice");
                }
            })
            .build(),
    )?;
    for sc in [workspace, voice] {
        if let Err(e) = app.global_shortcut().register(sc) {
            // Another app owns the combination; JARVIS stays usable via the
            // island and tray, and the conflict is visible in the log.
            tracing::warn!("global shortcut {sc:?} unavailable: {e}");
        }
    }
    Ok(())
}

pub fn run() {
    init_logging();
    tracing::info!("JARVIS {} starting", env!("CARGO_PKG_VERSION"));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            app.state::<AppState>().island.set_user_hidden(false);
            emit_command(app, "open-workspace");
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
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
                started_at: Instant::now(),
            });

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
            setup_shortcuts(app)?;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running JARVIS");
}
