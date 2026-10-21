//! Event-driven system watcher on a dedicated message-loop thread:
//!  * `EVENT_SYSTEM_FOREGROUND` WinEvent hook → foreground history and
//!    fullscreen detection (no polling of the foreground window);
//!  * a hidden top-level window receiving `WM_DISPLAYCHANGE` and work-area
//!    `WM_SETTINGCHANGE` broadcasts → re-place the island (debounced);
//!  * a slow 1.5 s timer re-checking fullscreen state, because apps toggling
//!    fullscreen in place (F11, video players) don't change the foreground.

use std::sync::{Arc, OnceLock};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, RegisterClassW,
    SetTimer, TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG, OBJID_WINDOW, SPI_SETWORKAREA,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_DISPLAYCHANGE, WM_SETTINGCHANGE, WM_TIMER,
    WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

use super::display;
use super::island::Island;
use super::winmgr::ForegroundHistory;

struct Ctx {
    island: Arc<Island>,
    history: Arc<ForegroundHistory>,
}

static CTX: OnceLock<Ctx> = OnceLock::new();

const TIMER_FULLSCREEN: usize = 1;
const TIMER_REPLACE: usize = 2;

fn check_fullscreen(ctx: &Ctx) {
    let fs = display::foreground_is_fullscreen(ctx.island.hwnd());
    ctx.island.set_fullscreen_hidden(fs);
}

unsafe extern "system" fn on_win_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != OBJID_WINDOW.0 {
        return;
    }
    if let Some(ctx) = CTX.get() {
        ctx.history.record(hwnd);
        check_fullscreen(ctx);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_DISPLAYCHANGE => {
            // Several broadcasts arrive per change; re-place once it settles.
            unsafe { SetTimer(Some(hwnd), TIMER_REPLACE, 400, None) };
            LRESULT(0)
        }
        WM_SETTINGCHANGE if wparam.0 as u32 == SPI_SETWORKAREA.0 => {
            unsafe { SetTimer(Some(hwnd), TIMER_REPLACE, 400, None) };
            LRESULT(0)
        }
        WM_TIMER => {
            if let Some(ctx) = CTX.get() {
                match wparam.0 {
                    TIMER_FULLSCREEN => check_fullscreen(ctx),
                    TIMER_REPLACE => {
                        let _ = unsafe { KillTimer(Some(hwnd), TIMER_REPLACE) };
                        if let Err(e) = ctx.island.place() {
                            tracing::warn!("re-placing island failed: {e}");
                        }
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn spawn(island: Arc<Island>, history: Arc<ForegroundHistory>) {
    if CTX.set(Ctx { island, history }).is_err() {
        tracing::warn!("system watcher already running");
        return;
    }
    std::thread::Builder::new()
        .name("system-watcher".into())
        .spawn(|| unsafe {
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if hook.is_invalid() {
                tracing::error!("SetWinEventHook failed; foreground tracking disabled");
            }

            let Ok(hinst) = GetModuleHandleW(None) else {
                tracing::error!("GetModuleHandleW failed; display watcher disabled");
                return;
            };
            let class = w!("JarvisSystemWatcher");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            // A hidden *top-level* window: message-only windows don't receive
            // broadcast messages such as WM_DISPLAYCHANGE.
            let hwnd = match CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class,
                w!("JARVIS watcher"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst.into()),
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!("watcher window creation failed: {e}");
                    return;
                }
            };
            SetTimer(Some(hwnd), TIMER_FULLSCREEN, 1500, None);

            if let Some(ctx) = CTX.get() {
                check_fullscreen(ctx);
            }

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn system watcher");
}
