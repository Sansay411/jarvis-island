//! What the setup and the uninstall windows share: one frameless window with
//! the page served from memory over `jarvis://`, the title-bar commands, and
//! the few Windows lookups both need (where Jarvis lives, known folders).

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::http::{header::CONTENT_TYPE, Response};
use wry::{WebContext, WebViewBuilder};

pub const RELEASE_URL: &str = "https://github.com/Sansay411/jarvis-island/releases/latest";
pub const LICENSE_URL: &str = "https://github.com/Sansay411/jarvis-island/blob/main/LICENSE";
pub const SOURCE_URL: &str = "https://github.com/Sansay411/jarvis-island";
pub const PRIVACY_URL: &str = "https://github.com/Sansay411/jarvis-island/blob/main/PRIVACY.md";

pub type Asset = (Cow<'static, [u8]>, &'static str);

pub enum Ui {
    Ipc(String),
    Eval(String),
}

/// Sends a script to the page from any thread.
#[derive(Clone)]
pub struct Page(EventLoopProxy<Ui>);

impl Page {
    pub fn eval(&self, js: impl Into<String>) {
        let _ = self.0.send_event(Ui::Eval(js.into()));
    }
}

pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Styles, fonts and pictures both windows use.
pub fn shared_asset(path: &str) -> Option<Asset> {
    let (bytes, mime): (&'static [u8], &'static str) = match path {
        "/app.css" => (include_bytes!("../ui/app.css"), "text/css; charset=utf-8"),
        "/hero.png" => (include_bytes!("../ui/hero.png"), "image/png"),
        "/scene.png" => (include_bytes!("../ui/scene.png"), "image/png"),
        "/onest-cyrillic.woff2" => (include_bytes!("../ui/onest-cyrillic.woff2"), "font/woff2"),
        "/onest-latin.woff2" => (include_bytes!("../ui/onest-latin.woff2"), "font/woff2"),
        _ => return None,
    };
    Some((Cow::Borrowed(bytes), mime))
}

/// Opens the window and runs until it closes. The shell handles the title
/// bar (`ready`, `drag`, `minimize`, `close`, `open:<url>`); every other
/// message goes to `on_ipc` as `(command, argument)`, which returns `true`
/// to close the window. WebView2 keeps its profile in `webview_data` (by
/// default it would sit next to the exe: in Downloads, or in the very folder
/// the uninstaller removes).
pub fn run<A, F>(title: &str, webview_data: PathBuf, assets: A, mut on_ipc: F) -> !
where
    A: Fn(&str) -> Option<Asset> + Send + Sync + 'static,
    F: FnMut(&str, &str, &Page) -> bool + 'static,
{
    let event_loop = EventLoopBuilder::<Ui>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let page = Page(proxy.clone());
    let builder = WindowBuilder::new()
        .with_title(title)
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
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let _ = late.send_event(Ui::Ipc("ready".into()));
    });

    // WebView2 processes of a window that has just closed can still hold the
    // profile («resource busy»); a fresh folder next to it always works.
    let assets = std::sync::Arc::new(assets);
    let spare = webview_data.with_extension(std::process::id().to_string());
    let [mut first, mut second] = [WebContext::new(Some(webview_data)), WebContext::new(Some(spare))];
    let build = |context: &mut WebContext| {
        let (assets, ipc) = (assets.clone(), proxy.clone());
        WebViewBuilder::new_with_web_context(context)
            .with_custom_protocol("jarvis".into(), move |_, req| {
                match assets(req.uri().path()).or_else(|| shared_asset(req.uri().path())) {
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
    };
    let webview = build(&mut first).or_else(|_| build(&mut second)).expect("webview");

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
                    "open" => open_url(match arg {
                        "license" => LICENSE_URL,
                        "source" => SOURCE_URL,
                        "privacy" => PRIVACY_URL,
                        _ => RELEASE_URL,
                    }),
                    _ => {
                        if on_ipc(cmd, arg, &page) {
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    })
}

#[cfg(windows)]
fn round_corners(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE};
    let pref = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(HWND(hwnd as _), DWMWA_WINDOW_CORNER_PREFERENCE, (&pref as *const DWM_WINDOW_CORNER_PREFERENCE).cast(), std::mem::size_of_val(&pref) as u32);
    }
}

/// Opens a link in the default browser through the shell, the way Explorer does.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    unsafe {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
    }
    #[cfg(not(windows))]
    let _ = url;
}

/// Where the NSIS package put Jarvis: it writes the folder to
/// `HKCU\Software\jarvis\Jarvis`; the default per-user folder otherwise.
#[cfg(windows)]
pub fn install_dir() -> Option<PathBuf> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!("Software\\jarvis\\Jarvis"), None, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) }.is_ok();
    if ok {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        return Some(PathBuf::from(String::from_utf16_lossy(&buf[..end]).trim_matches('"')));
    }
    Some(known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)?.join("Jarvis"))
}

#[cfg(not(windows))]
pub fn install_dir() -> Option<PathBuf> {
    None
}

pub fn installed_exe() -> Option<PathBuf> {
    let exe = install_dir()?.join("jarvis.exe");
    exe.exists().then_some(exe)
}

/// A folder Windows knows by id (OneDrive and policies move them, so never
/// guess from environment variables).
#[cfg(windows)]
pub fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as _));
        path
    }
}

/// The user's desktop, wherever Windows keeps it.
pub fn desktop_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return known_folder(&windows::Win32::UI::Shell::FOLDERID_Desktop);
    #[cfg(not(windows))]
    None
}

/// Total size of a folder, without following links or junctions.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return meta.len();
    }
    std::fs::read_dir(path).map(|it| it.flatten().map(|e| dir_size(&e.path())).sum()).unwrap_or(0)
}

/// Where the uninstall window runs from: a copy outside the install folder,
/// so the folder itself can go. The setup clears it on its next run.
pub fn uninstall_temp_dir() -> PathBuf {
    std::env::temp_dir().join("JarvisUninstall")
}
