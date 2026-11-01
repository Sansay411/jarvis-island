//! Clipboard change notifications: a hidden message-only window registered
//! with `AddClipboardFormatListener`, so nothing polls. One copy often posts
//! several updates (programs add formats one by one, delayed rendering), so
//! a short timer collapses them into a single callback.

use std::sync::OnceLock;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::AddClipboardFormatListener;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, RegisterClassW, SetTimer,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_TIMER, WNDCLASSW,
};

type Callback = Box<dyn Fn() + Send + Sync>;

static ON_CHANGE: OnceLock<Callback> = OnceLock::new();

const TIMER_SETTLE: usize = 1;
const SETTLE_MS: u32 = 150;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Re-arming the same timer restarts it: the callback fires once, after
        // the last update of a burst.
        WM_CLIPBOARDUPDATE => {
            unsafe { SetTimer(Some(hwnd), TIMER_SETTLE, SETTLE_MS, None) };
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_SETTLE => {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_SETTLE) };
            if let Some(on_change) = ON_CHANGE.get() {
                on_change();
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Start listening. `on_change` runs on the watcher thread, so it should
/// hand the work off rather than do it.
pub fn spawn(on_change: impl Fn() + Send + Sync + 'static) {
    if ON_CHANGE.set(Box::new(on_change)).is_err() {
        tracing::warn!("clipboard watcher already running");
        return;
    }
    std::thread::Builder::new()
        .name("clipboard-watcher".into())
        .spawn(|| unsafe {
            let Ok(hinst) = GetModuleHandleW(None) else {
                tracing::error!("GetModuleHandleW failed; clipboard history disabled");
                return;
            };
            let class = w!("JarvisClipboardWatcher");
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() });
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("JARVIS clipboard"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinst.into()),
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!("clipboard watcher window creation failed: {e}");
                    return;
                }
            };
            if let Err(e) = AddClipboardFormatListener(hwnd) {
                tracing::error!("AddClipboardFormatListener failed: {e}");
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn clipboard watcher");
}
