//! Configurable global shortcuts (RegisterHotKey via the global-shortcut
//! plugin). A combination owned by another app is logged and reported, and
//! JARVIS stays usable from the island and tray.

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::platform::island;
use crate::state::AppState;
use crate::store::settings::ShortcutSettings;

pub const EVT_COMMAND: &str = "island:command";

#[derive(Default)]
struct Bound {
    workspace: Option<Shortcut>,
    voice: Option<Shortcut>,
}

static BOUND: Mutex<Bound> = Mutex::new(Bound { workspace: None, voice: None });
static PROBLEMS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn emit_command(app: &AppHandle, action: &str) {
    let _ = app.emit_to(island::LABEL, EVT_COMMAND, action);
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            app.state::<AppState>().island.set_user_hidden(false);
            let b = BOUND.lock();
            if b.workspace.as_ref() == Some(shortcut) {
                emit_command(app, "toggle-workspace");
            } else if b.voice.as_ref() == Some(shortcut) {
                let voice = app.state::<std::sync::Arc<crate::voice::VoiceManager>>();
                if let Err(e) = voice.toggle() {
                    tracing::warn!("voice start failed: {e}");
                    let _ = app.emit(crate::voice::EVT_STATE, serde_json::json!({ "state": "error", "error": e.to_string() }));
                }
            }
        })
        .build()
}

fn parse(s: &str) -> Option<Shortcut> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<Shortcut>().map_err(|e| tracing::warn!("bad shortcut {s:?}: {e}")).ok()
}

/// (Re)register shortcuts; returns human-readable problems.
pub fn apply(app: &AppHandle, cfg: &ShortcutSettings) -> Vec<String> {
    let gs = app.global_shortcut();
    let mut b = BOUND.lock();
    for sc in [b.workspace.take(), b.voice.take()].into_iter().flatten() {
        let _ = gs.unregister(sc);
    }
    let mut problems = Vec::new();
    let mut register = |label: &str, text: &str| -> Option<Shortcut> {
        let sc = match parse(text) {
            Some(sc) => sc,
            None if text.trim().is_empty() => return None,
            None => {
                problems.push(format!("{label}: не удалось разобрать «{text}»"));
                return None;
            }
        };
        match gs.register(sc) {
            Ok(()) => Some(sc),
            Err(e) => {
                tracing::warn!("shortcut {text} unavailable: {e}");
                problems.push(format!("{label}: «{text}» уже занято другой программой"));
                None
            }
        }
    };
    b.workspace = register("Чат", &cfg.workspace);
    b.voice = register("Голос", &cfg.voice);
    *PROBLEMS.lock() = problems.clone();
    problems
}

pub fn problems() -> Vec<String> {
    PROBLEMS.lock().clone()
}
