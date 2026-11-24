//! JARVIS — native AI desktop agent for Windows 11.
//!
//! Layering (see docs/ARCHITECTURE.md):
//!   platform  → Win32/COM/WinRT integrations, no UI knowledge
//!   store     → SQLite settings/history/audit, Credential Manager secrets
//!   ai        → provider adapters (OpenAI-compatible)
//!   tools     → registry of verified agent capabilities
//!   mcp       → client for the MCP servers the user connected
//!   agent     → the Observe → Reason → Act → Verify loop
//!   ipc*      → Tauri commands/events, thin adapters
//!   state     → shared handles managed by Tauri

pub mod agent;
pub mod ai;
pub mod bluetooth;
pub mod cloud;
pub mod clips;
pub mod error;
pub mod filekit;
pub mod geo;
pub mod github;
pub mod hub;
mod ipc;
mod ipc_clips;
mod ipc_files;
mod ipc_hub;
pub mod jobs;
mod ipc_mcp;
mod ipc_mentor;
mod ipc_planner;
pub mod ipc_tts;
pub mod mcp;
pub mod mentor;
mod ipc_agent;
pub mod overlay;
pub mod planner;
pub mod platform;
pub mod reminders;
pub mod stopwatch;
pub mod telegram;
mod shortcuts;
mod state;
pub mod store;
pub mod tools;
pub mod updater;
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
    let update = MenuItem::with_id(app, "update", "Проверить обновления", true, None::<&str>)?;
    let cancel = MenuItem::with_id(app, "cancel", "Остановить текущую задачу", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти из JARVIS", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&chat, &toggle, &sep, &cancel, &settings, &update, &sep2, &quit])?;
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
            "update" => {
                // Found: install it (the user asked from the tray). Not found:
                // the island says so.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let updates = app.state::<updater::Updates>();
                    match updates.check(&app).await {
                        Ok(Some(_)) => {
                            if let Err(e) = updates.install(&app).await {
                                app.state::<Arc<agent::Agent>>().post_report(e.to_string(), false);
                            }
                        }
                        Ok(None) => app.state::<Arc<agent::Agent>>().post_report(format!("Установлена последняя версия Джарвиса — {}.", env!("CARGO_PKG_VERSION")), true),
                        Err(e) => app.state::<Arc<agent::Agent>>().post_report(e.to_string(), false),
                    }
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
            // Another program can send this, so the user always sees it. Only
            // debug builds may run it hidden (`--quiet`, for the e2e scripts).
            if !(cfg!(debug_assertions) && argv.iter().any(|a| a == "--quiet")) {
                app.state::<AppState>().island.set_user_hidden(false);
                emit_command(app, "open-workspace");
            }
            let agent = app.state::<Arc<agent::Agent>>();
            if let Err(e) = agent.send_external(text) {
                tracing::warn!("--ask rejected: {e}");
                agent.post_report(format!("Не удалось начать задачу: {e}"), false);
            }
        }
        return true;
    }
    // `--conversation ID`: continue that conversation (tests restore the
    // user's own after running in a fresh one).
    if let Some(pos) = argv.iter().position(|a| a == "--conversation") {
        if let Some(id) = argv.get(pos + 1) {
            if let Err(e) = app.state::<Arc<agent::Agent>>().use_conversation(id.clone()) {
                tracing::warn!("--conversation: {e}");
            }
        }
        return true;
    }
    // `--command toggle-workspace|open-workspace|toggle-voice|cancel-task|
    // new-conversation` lets launchers, Stream Deck buttons and scripts
    // drive the island.
    if let Some(pos) = argv.iter().position(|a| a == "--command") {
        if let Some(cmd) = argv.get(pos + 1) {
            if cmd == "cancel-task" {
                app.state::<Arc<agent::Agent>>().cancel();
            } else if cmd == "quit" {
                app.state::<Arc<voice::VoiceManager>>().stop();
                app.state::<Arc<agent::Agent>>().cancel();
                let handle=app.clone();
                tauri::async_runtime::spawn(async move {tokio::time::sleep(Duration::from_millis(500)).await;handle.exit(0);});
            } else if cmd == "stop-voice" {
                app.state::<Arc<voice::VoiceManager>>().stop();
            } else if cmd == "new-conversation" {
                if let Err(e) = app.state::<Arc<agent::Agent>>().new_conversation() {
                    tracing::warn!("new-conversation: {e}");
                }
            } else if matches!(cmd.as_str(), "toggle-workspace" | "open-workspace" | "toggle-voice") {
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

/// Makes the Windows Run entry match the setting and point at *this* exe.
/// The entry used to be written only when the toggle flipped, so it kept the
/// path of whichever build flipped it (once `target\debug\jarvis.exe`, which
/// boots into a dead dev URL). Debug builds never touch it.
pub(crate) fn sync_autostart(app: &tauri::AppHandle, on: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        tracing::info!("autostart left alone in a debug build (wanted {on})");
        return Ok(());
    }
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    let res = if on {
        al.enable()
    } else if al.is_enabled().unwrap_or(true) {
        al.disable()
    } else {
        Ok(())
    };
    res.map_err(|e| e.to_string())
}

pub fn run() {
    init_logging();
    // `--probe-mic NAME`: a helper process started by the running app to
    // check the microphone outside its own audio stack (voice::heal_audio).
    // Exits before the single-instance plugin, so it never reaches the app.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(pos) = argv.iter().position(|a| a == "--probe-mic") {
        let name = argv.get(pos + 1).cloned().unwrap_or_default();
        let res = voice::audio::probe_input(&name);
        tracing::info!("microphone probe: {res:?}");
        std::process::exit(if res.is_ok() { 0 } else { 1 });
    }
    tracing::info!("JARVIS {} starting", env!("CARGO_PKG_VERSION"));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // A second launch (Start menu, shortcut, `jarvis.exe --settings`)
            // surfaces the running instance instead of starting another.
            shortcuts::launch(app, argv);
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(shortcuts::plugin())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            let mcp = mcp::Hub::new(app.handle().clone());
            mcp.spawn_background();
            app.manage(mcp.clone());
            let registry = Arc::new(tools::Registry::new(mcp));
            app.manage(agent::Agent::new(app.handle().clone(), registry));
            let hub = hub::Hub::new(app.handle().clone())?;
            hub.spawn();
            app.manage(hub);
            let planner = planner::Planner::new(app.handle().clone(), app.state::<AppState>().store.clone());
            planner::google::spawn(planner.clone());
            app.manage(planner);
            let mentor = mentor::Mentor::new(Some(app.handle().clone()), app.state::<AppState>().store.clone())?;
            mentor::guard::spawn(app.handle().clone(), mentor.clone());
            app.manage(mentor);
            let bot = Arc::new(telegram::Bot::default());
            telegram::spawn(app.handle().clone(), bot.clone());
            app.manage(bot);
            app.manage(jobs::JobManager::new(app.handle().clone()));
            app.manage(voice::VoiceManager::new(app.handle().clone()));
            app.manage(Arc::new(filekit::FileJobs::default()));
            let reminders = reminders::Reminders::new(app.handle().clone());
            app.manage(reminders.clone());
            app.manage(Arc::new(stopwatch::Stopwatch::new(app.handle().clone())));
            app.manage(updater::Updates::default());
            updater::spawn(app.handle().clone());
            reminders.spawn();
            let clips = clips::Clips::new(app.handle().clone());
            app.manage(clips.clone());
            clips.start();
            let bluetooth = bluetooth::Bluetooth::new(app.handle().clone());
            app.manage(bluetooth.clone());
            // After start-up has settled: nobody waits for the first battery read.
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                bluetooth.spawn();
            });
            // Start listening for «Джарвис» if that is on — after WebView2 has
            // booted and painted the island, so model loading does not pile
            // onto the busiest second of start-up.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(2500)).await;
                    handle.state::<Arc<voice::VoiceManager>>().apply_settings();
                });
            }

            window.show()?;
            island.spawn_tracker();
            platform::watcher::spawn(island.clone(), history);
            if let Err(e) = platform::media::MediaWatcher::spawn(app.handle().clone()) {
                tracing::warn!("media watcher not started: {e}");
            }
            // Warm the app index in the background so the first "open X"
            // doesn't pay for Start-menu enumeration — after the speech models
            // have loaded, at background priority.
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(12)).await;
                let _ = platform::blocking(move || platform::in_background(|| apps.ensure_fresh(Duration::from_secs(600)))).await;
            });

            if let Err(e) = sync_autostart(app.handle(), settings.launch_at_startup) {
                tracing::warn!("autostart not synced: {e}");
            }
            setup_tray(app)?;
            // First launch with CLI arguments: act once the island UI is listening.
            let argv: Vec<String> = std::env::args().collect();
            if argv.len() > 1 {
                shortcuts::launch(app.handle(), argv);
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
                WindowEvent::DragDrop(e) => tracing::debug!(?e, "island drag-drop"),
                WindowEvent::ScaleFactorChanged { .. } => {
                    let _ = window.state::<AppState>().island.place();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            shortcuts::island_frontend_ready,
            ipc::island_set_shape,
            ipc::island_notch_geometry,
            ipc::island_begin_input,
            ipc::island_end_input,
            ipc::island_set_hidden,
            ipc::island_context,
            ipc::perf_report,
            ipc::update_status,
            ipc::update_check,
            ipc::update_install,
            ipc::bluetooth_devices,
            ipc::bluetooth_refresh,
            ipc::audio_get,
            ipc::audio_set_volume,
            ipc::audio_set_mute,
            ipc::brightness_list,
            ipc::brightness_set,
            ipc::media_get,
            ipc::media_artwork,
            ipc::media_app_icon,
            ipc::media_control,
            ipc::media_seek,
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
            ipc::stt_models,
            ipc::stt_download,
            ipc::stt_download_cancel,
            ipc::tts_voices,
            ipc::tts_preview,
            ipc::job_cancel,
            ipc_files::files_inspect,
            ipc_files::files_action,
            ipc_files::files_cancel,
            ipc_files::files_clear,
            ipc_files::reminders_list,
            ipc_files::reminder_done,
            ipc_files::reminder_snooze,
            ipc_files::reminder_extend,
            ipc_files::stopwatch_get,
            ipc_files::stopwatch_action,
            ipc_clips::clips_list,
            ipc_clips::clips_status,
            ipc_clips::clips_set_enabled,
            ipc_clips::clips_paste,
            ipc_clips::clips_copy,
            ipc_clips::clips_pin,
            ipc_clips::clips_delete,
            ipc_clips::clips_clear,
            ipc_agent::agent_status,
            ipc_agent::agent_send,
            ipc_agent::agent_cancel,
            ipc_agent::agent_confirm,
            ipc_agent::agent_new_conversation,
            ipc_agent::agent_history,
            ipc_agent::agent_set_full_control,
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
            ipc_mcp::mcp_list,
            ipc_mcp::mcp_save,
            ipc_mcp::mcp_import,
            ipc_mcp::mcp_remove,
            ipc_mcp::mcp_refresh,
            ipc_mcp::mcp_set_enabled,
            ipc_mcp::mcp_set_tool,
            ipc_hub::hub_tasks_list,
            ipc_hub::hub_task_add,
            ipc_hub::hub_task_toggle,
            ipc_hub::hub_task_remove,
            ipc_hub::hub_focus_get,
            ipc_hub::hub_focus_start,
            ipc_hub::hub_focus_pause_toggle,
            ipc_hub::hub_focus_complete,
            ipc_hub::hub_focus_cancel,
            ipc_hub::hub_shelf_list,
            ipc_hub::hub_shelf_add_files,
            ipc_hub::hub_shelf_add_text,
            ipc_hub::hub_shelf_remove,
            ipc_hub::hub_shelf_clear,
            ipc_hub::hub_shelf_thumbnail,
            ipc_hub::hub_shelf_drag_out,
            ipc_hub::hub_activity_get,
            ipc_hub::hub_github_get,
            ipc_hub::hub_github_set_login,
            ipc_hub::weather_get,
            ipc_hub::hub_routines_list,
            ipc_hub::hub_routine_save,
            ipc_hub::hub_routine_remove,
            ipc_hub::hub_routine_run,
            ipc_tts::tts_speak,
            ipc_tts::tts_stop,
            ipc_tts::tts_status,
            ipc_tts::tts_benchmark,
            ipc_planner::planner_open,
            ipc_planner::planner_events,
            ipc_planner::planner_create,
            ipc_planner::planner_update,
            ipc_planner::planner_delete,
            ipc_planner::planner_suggest,
            ipc_planner::planner_google_status,
            ipc_planner::planner_google_connect,
            ipc_planner::planner_google_disconnect,
            ipc_planner::planner_google_sync,
            ipc_mentor::mentor_state,
            ipc_mentor::mentor_pending,
            ipc_mentor::mentor_open_reset,
            ipc_mentor::mentor_feed,
            ipc_mentor::mentor_answer,
            ipc_mentor::mentor_complete,
            ipc_mentor::mentor_best,
            ipc_mentor::mentor_workout,
            ipc_mentor::mentor_words,
            ipc_mentor::mentor_generate,
            ipc_mentor::mentor_steps,
            ipc_mentor::mentor_start_step,
            ipc_mentor::mentor_allow,
            ipc_mentor::mentor_review,
            ipc_mentor::telegram_status,
            ipc_mentor::telegram_set_token,
            ipc_mentor::telegram_pair,
            ipc_mentor::telegram_unbind,
            overlay::overlay_ready,
            overlay::overlay_set_interactive_rects,
            overlay::overlay_dismiss_item,
            overlay::overlay_cancel_guide,
        ])
        .run(tauri::generate_context!())
        .expect("error while running JARVIS");
}

#[cfg(all(test, windows))]
mod test_comctl6_manifest {
    // `tauri_build` embeds the Common Controls v6 manifest into `bins` only;
    // `--lib` unit test executables also link `tao` (`TaskDialogIndirect`).
    const DRECTVE: &[u8] = b" /MANIFESTDEPENDENCY:\"type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'\" ";
    #[used]
    #[link_section = ".drectve"]
    static LINK_ARGS: [u8; DRECTVE.len()] = {
        let mut out = [0u8; DRECTVE.len()];
        let mut i = 0;
        while i < DRECTVE.len() {
            out[i] = DRECTVE[i];
            i += 1;
        }
        out
    };
}
