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

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::http::{header::CONTENT_TYPE, Response};
use wry::WebViewBuilder;

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.exe"));
const VERSION: &str = env!("JARVIS_VERSION");
const RELEASE_URL: &str = "https://github.com/Sansay411/jarvis-island/releases/latest";
const LICENSE_URL: &str = "https://github.com/Sansay411/jarvis-island/blob/main/LICENSE";
const SOURCE_URL: &str = "https://github.com/Sansay411/jarvis-island";

enum Ui {
    Ipc(String),
    Eval(String),
}

fn asset(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let (bytes, mime): (&'static [u8], &'static str) = match path {
        "/" | "/index.html" => (include_bytes!("../ui/index.html"), "text/html; charset=utf-8"),
        "/app.css" => (include_bytes!("../ui/app.css"), "text/css; charset=utf-8"),
        "/app.js" => (include_bytes!("../ui/app.js"), "text/javascript; charset=utf-8"),
        "/hero.png" => (include_bytes!("../ui/hero.png"), "image/png"),
        "/scene.png" => (include_bytes!("../ui/scene.png"), "image/png"),
        "/onest-cyrillic.woff2" => (include_bytes!("../ui/onest-cyrillic.woff2"), "font/woff2"),
        "/onest-latin.woff2" => (include_bytes!("../ui/onest-latin.woff2"), "font/woff2"),
        "/config.js" => {
            let js = format!("window.SETUP={};", serde_json::json!({ "version": VERSION, "payload": !PAYLOAD.is_empty() }));
            return Some((Cow::Owned(js.into_bytes()), "text/javascript; charset=utf-8"));
        }
        _ => return None,
    };
    Some((Cow::Borrowed(bytes), mime))
}

/// Where the package put Jarvis: the NSIS installer writes the folder to
/// `HKCU\Software\jarvis\Jarvis`; the default per-user folder otherwise.
#[cfg(windows)]
fn installed_exe() -> Option<PathBuf> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!("Software\\jarvis\\Jarvis"), None, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) }.is_ok();
    let dir = if ok {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        PathBuf::from(String::from_utf16_lossy(&buf[..end]).trim_matches('"'))
    } else {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Jarvis")
    };
    let exe = dir.join("jarvis.exe");
    exe.exists().then_some(exe)
}

#[cfg(not(windows))]
fn installed_exe() -> Option<PathBuf> {
    None
}

/// The user's desktop, wherever Windows keeps it (OneDrive moves it).
#[cfg(windows)]
fn desktop_dir() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as _));
        path
    }
}

#[cfg(not(windows))]
fn desktop_dir() -> Option<PathBuf> {
    None
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Run the package silently while the window shows the steps. NSIS says
/// nothing about its progress, so the bar follows a curve that slows towards
/// the end and jumps to 100 % when the package reports success.
fn install(proxy: EventLoopProxy<Ui>, desktop_shortcut: bool) {
    std::thread::spawn(move || {
        let send = |js: String| {
            let _ = proxy.send_event(Ui::Eval(js));
        };
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

fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
}

#[cfg(windows)]
fn round_corners(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    let pref = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(HWND(hwnd as _), DWMWA_WINDOW_CORNER_PREFERENCE, (&pref as *const windows::Win32::Graphics::Dwm::DWM_WINDOW_CORNER_PREFERENCE).cast(), std::mem::size_of_val(&pref) as u32);
    }
}

fn main() {
    let event_loop = EventLoopBuilder::<Ui>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let builder = WindowBuilder::new()
        .with_title("Установка Джарвиса")
        .with_inner_size(LogicalSize::new(920.0, 640.0))
        .with_resizable(false)
        .with_maximizable(false)
        .with_decorations(false)
        .with_visible(false);
    #[cfg(windows)]
    let builder = {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder.with_undecorated_shadow(true)
    };
    let window = builder.build(&event_loop).expect("window");
    if let Some(monitor) = window.current_monitor() {
        let (m, s) = (monitor.position(), monitor.size());
        let w = window.outer_size();
        window.set_outer_position(tao::dpi::PhysicalPosition::new(m.x + (s.width as i32 - w.width as i32) / 2, m.y + (s.height as i32 - w.height as i32) / 2));
    }
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowExtWindows;
        round_corners(window.hwnd());
    }

    // Shown once the page has painted; a second and a half at most.
    let late = proxy.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let _ = late.send_event(Ui::Ipc("ready".into()));
    });

    let ipc = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_custom_protocol("jarvis".into(), |_, req| {
            match asset(req.uri().path()) {
                Some((body, mime)) => Response::builder().header(CONTENT_TYPE, mime).body(body),
                None => Response::builder().status(404).body(Cow::Borrowed(&[][..])),
            }
            .unwrap()
        })
        .with_url("jarvis://localhost/")
        .with_background_color((243, 244, 246, 255))
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |req| {
            let _ = ipc.send_event(Ui::Ipc(req.body().clone()));
        })
        .build(&window)
        .expect("webview");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Ui::Eval(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(Ui::Ipc(msg)) => {
                let (cmd, arg) = msg.split_once(':').unwrap_or((msg.as_str(), ""));
                match cmd {
                    "ready" => window.set_visible(true),
                    "drag" => {
                        let _ = window.drag_window();
                    }
                    "minimize" => window.set_minimized(true),
                    "close" => *control_flow = ControlFlow::Exit,
                    "install" => install(proxy.clone(), arg == "1"),
                    "launch" => {
                        if !arg.is_empty() {
                            let _ = Command::new(arg).spawn();
                        }
                        *control_flow = ControlFlow::Exit;
                    }
                    "open" => open_url(match arg {
                        "license" => LICENSE_URL,
                        "source" => SOURCE_URL,
                        _ => RELEASE_URL,
                    }),
                    _ => {}
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}
