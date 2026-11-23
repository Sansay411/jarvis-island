#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Jarvis's installer: one window in the style of a modern product installer
//! (a welcome screen with the hero, then the progress with its steps) that
//! installs the app with the NSIS package Tauri builds. The package is
//! embedded at compile time and run silently, so the uninstaller, the Start
//! menu entry and the auto-updater keep working exactly as before.

use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use jarvis_setup::{desktop_dir, installed_exe, js_str, uninstall_temp_dir, Asset, Page};

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.exe"));
const VERSION: &str = env!("JARVIS_VERSION");

fn asset(path: &str) -> Option<Asset> {
    let (bytes, mime): (&'static [u8], &'static str) = match path {
        "/" | "/index.html" => (include_bytes!("../ui/index.html"), "text/html; charset=utf-8"),
        "/app.js" => (include_bytes!("../ui/app.js"), "text/javascript; charset=utf-8"),
        "/config.js" => {
            let js = format!("window.SETUP={};", serde_json::json!({ "version": VERSION, "payload": !PAYLOAD.is_empty() }));
            return Some((Cow::Owned(js.into_bytes()), "text/javascript; charset=utf-8"));
        }
        _ => return None,
    };
    Some((Cow::Borrowed(bytes), mime))
}

/// Run the package silently while the window shows the steps. NSIS says
/// nothing about its progress, so the bar follows a curve that slows towards
/// the end and jumps to 100 % when the package reports success.
fn install(page: Page, desktop_shortcut: bool) {
    std::thread::spawn(move || {
        let send = |js: String| page.eval(js);
        let result = (|| -> Result<PathBuf, String> {
            if PAYLOAD.is_empty() {
                for p in (0..=96).step_by(4) {
                    send(format!("window.setup.progress({p})"));
                    std::thread::sleep(Duration::from_millis(220));
                }
                return Err("В этой сборке установщика нет самого Джарвиса.".into());
            }
            let dir = std::env::temp_dir().join(format!("JarvisSetup-{VERSION}"));
            std::fs::create_dir_all(&dir).map_err(|e| format!("Не удалось подготовить папку: {e}"))?;
            let package = dir.join("Jarvis-package.exe");
            std::fs::write(&package, PAYLOAD).map_err(|e| format!("Не удалось распаковать: {e}"))?;
            send("window.setup.step(1)".into());
            let mut child = Command::new(&package).arg("/S").spawn().map_err(|e| format!("Не удалось запустить установку: {e}"))?;
            let start = Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    break status;
                }
                let t = start.elapsed().as_secs_f64();
                send(format!("window.setup.progress({:.0})", 8.0 + 86.0 * (1.0 - (-t / 7.0).exp())));
                std::thread::sleep(Duration::from_millis(120));
            };
            let _ = std::fs::remove_file(&package);
            let _ = std::fs::remove_dir(&dir);
            if !status.success() {
                return Err(format!("Установщик пакета завершился с кодом {}.", status.code().unwrap_or(-1)));
            }
            send("window.setup.step(2)".into());
            if !desktop_shortcut {
                if let Some(d) = desktop_dir() {
                    let _ = std::fs::remove_file(d.join("Jarvis.lnk"));
                }
            }
            installed_exe().ok_or_else(|| "Джарвис установлен, но не нашёлся на диске.".to_string())
        })();
        match result {
            Ok(exe) => send(format!("window.setup.done({})", js_str(&exe.to_string_lossy()))),
            Err(e) => send(format!("window.setup.failed({})", js_str(&e))),
        }
    });
}

fn main() {
    // What a previous uninstall left running from %TEMP%.
    let _ = std::fs::remove_dir_all(uninstall_temp_dir());
    let webview = std::env::temp_dir().join("JarvisSetup-webview");
    jarvis_setup::run("Установка Джарвиса", webview, asset, |cmd, arg, page| match cmd {
        "install" => {
            install(page.clone(), arg == "1");
            false
        }
        "launch" => {
            if !arg.is_empty() {
                let _ = Command::new(arg).spawn();
            }
            true
        }
        _ => false,
    })
}
