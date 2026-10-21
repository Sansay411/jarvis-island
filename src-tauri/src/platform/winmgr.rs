//! Top-level window enumeration and management (focus, minimise, restore,
//! close, move/resize, snapping) plus a short history of foreground windows
//! used for "go back to the previous window" and agent context.

use std::collections::VecDeque;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VK_LMENU, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, IsZoomed, PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, GW_OWNER, SWP_NOACTIVATE, SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    WM_CLOSE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

use super::display::{self, Rect};
use super::process::process_name;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// Window handle as an integer; stable while the window exists.
    pub hwnd: i64,
    pub title: String,
    pub process: String,
    pub pid: u32,
    pub class_name: String,
    /// "normal" | "minimized" | "maximized"
    pub state: &'static str,
    pub foreground: bool,
    pub rect: Rect,
    pub monitor: String,
}

pub fn to_hwnd(h: i64) -> HWND {
    HWND(h as isize as *mut _)
}

fn title_of(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
    }
}

/// Mirrors the Alt+Tab heuristic: visible, uncloaked, titled, unowned
/// application windows.
fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || is_cloaked(hwnd) {
            return false;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let app_window = ex & WS_EX_APPWINDOW.0 != 0;
        if ex & WS_EX_TOOLWINDOW.0 != 0 && !app_window {
            return false;
        }
        let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
        if !owner.is_invalid() && !app_window {
            return false;
        }
        GetWindowTextLengthW(hwnd) > 0
    }
}

pub fn describe(hwnd: HWND) -> Option<WindowInfo> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let pid = pid_of(hwnd);
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        let state = if IsIconic(hwnd).as_bool() {
            "minimized"
        } else if IsZoomed(hwnd).as_bool() {
            "maximized"
        } else {
            "normal"
        };
        Some(WindowInfo {
            hwnd: hwnd.0 as isize as i64,
            title: title_of(hwnd),
            process: process_name(pid).unwrap_or_default(),
            pid,
            class_name: class_of(hwnd),
            state,
            foreground: GetForegroundWindow() == hwnd,
            rect: r.into(),
            monitor: display::monitor_for_window(hwnd).map(|m| m.name).unwrap_or_default(),
        })
    }
}

/// All application windows in Z-order (topmost first), excluding JARVIS.
pub fn list_windows() -> Vec<WindowInfo> {
    unsafe extern "system" fn cb(hwnd: HWND, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
        out.push(hwnd);
        true.into()
    }
    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut handles as *mut _ as isize));
    }
    let own = unsafe { GetCurrentProcessId() };
    handles
        .into_iter()
        .filter(|&h| is_app_window(h) && pid_of(h) != own)
        .filter_map(describe)
        .collect()
}

pub fn foreground_window() -> Option<WindowInfo> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return None;
    }
    describe(fg)
}

fn require(hwnd: i64) -> AppResult<HWND> {
    let h = to_hwnd(hwnd);
    if unsafe { IsWindow(Some(h)).as_bool() } {
        Ok(h)
    } else {
        Err(AppError::not_found("Окно уже закрыто или не существует"))
    }
}

fn send_alt_tap() {
    unsafe {
        let sc = MapVirtualKeyW(VK_MENU.0 as u32, MAPVK_VK_TO_VSC) as u16;
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: VK_LMENU, wScan: sc, dwFlags: flags, time: 0, dwExtraInfo: 0 },
            },
        };
        let inputs = [key(KEYEVENTF_EXTENDEDKEY), key(KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP)];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Bring a window to the foreground and verify that it actually got there.
pub fn focus(hwnd: i64) -> AppResult<WindowInfo> {
    let h = require(hwnd)?;
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        if !SetForegroundWindow(h).as_bool() {
            // Windows restricts foreground changes; temporarily sharing the
            // input state with the current foreground thread is the
            // documented-compatible way to be allowed to switch.
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
            let _ = BringWindowToTop(h);
            let ok = SetForegroundWindow(h).as_bool();
            if attached {
                let _ = AttachThreadInput(me, fg_thread, false);
            }
            if !ok {
                send_alt_tap();
                let _ = SetForegroundWindow(h);
            }
        }
    }
    std::thread::sleep(Duration::from_millis(60));
    let info = describe(h).ok_or_else(|| AppError::not_found("Окно закрылось"))?;
    if !info.foreground {
        return Err(AppError::platform(format!(
            "Windows не позволила переключиться на «{}»",
            info.title
        )));
    }
    Ok(info)
}

pub fn minimize(hwnd: i64) -> AppResult<WindowInfo> {
    let h = require(hwnd)?;
    unsafe {
        let _ = ShowWindow(h, SW_MINIMIZE);
    }
    std::thread::sleep(Duration::from_millis(120));
    describe(h).ok_or_else(|| AppError::not_found("Окно закрылось"))
}

pub fn maximize(hwnd: i64) -> AppResult<WindowInfo> {
    let h = require(hwnd)?;
    unsafe {
        let _ = ShowWindow(h, SW_MAXIMIZE);
    }
    std::thread::sleep(Duration::from_millis(120));
    describe(h).ok_or_else(|| AppError::not_found("Окно закрылось"))
}

pub fn restore(hwnd: i64) -> AppResult<WindowInfo> {
    let h = require(hwnd)?;
    unsafe {
        let _ = ShowWindow(h, SW_RESTORE);
    }
    std::thread::sleep(Duration::from_millis(120));
    describe(h).ok_or_else(|| AppError::not_found("Окно закрылось"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseOutcome {
    pub closed: bool,
    /// The app is still open — usually because it asked about unsaved work.
    pub still_open_reason: Option<String>,
}

/// Politely ask a window to close (WM_CLOSE) so the application can prompt
/// about unsaved work. Never terminates the process.
pub fn close(hwnd: i64) -> AppResult<CloseOutcome> {
    let h = require(hwnd)?;
    unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0))? };
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        if !unsafe { IsWindow(Some(h)).as_bool() } || !unsafe { IsWindowVisible(h).as_bool() } {
            return Ok(CloseOutcome { closed: true, still_open_reason: None });
        }
    }
    Ok(CloseOutcome {
        closed: false,
        still_open_reason: Some(
            "Окно осталось открытым — вероятно, приложение спрашивает о несохранённых изменениях".into(),
        ),
    })
}

/// Difference between GetWindowRect and the visible frame (invisible resize
/// borders on Windows 10/11), so placement lines up with what users see.
fn frame_insets(h: HWND) -> (i32, i32, i32, i32) {
    unsafe {
        let mut outer = RECT::default();
        let mut visible = RECT::default();
        if GetWindowRect(h, &mut outer).is_err()
            || DwmGetWindowAttribute(
                h,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut visible as *mut RECT as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
        {
            return (0, 0, 0, 0);
        }
        (
            visible.left - outer.left,
            visible.top - outer.top,
            outer.right - visible.right,
            outer.bottom - visible.bottom,
        )
    }
}

/// Move/resize so the *visible* frame matches the requested rect (physical px).
pub fn move_resize(hwnd: i64, rect: Rect) -> AppResult<WindowInfo> {
    let h = require(hwnd)?;
    if rect.width < 100 || rect.height < 60 {
        return Err(AppError::invalid("Слишком маленький размер окна"));
    }
    unsafe {
        if IsZoomed(h).as_bool() || IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let (l, t, r, b) = frame_insets(h);
        SetWindowPos(
            h,
            None,
            rect.x - l,
            rect.y - t,
            rect.width + l + r,
            rect.height + t + b,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
    }
    std::thread::sleep(Duration::from_millis(80));
    describe(h).ok_or_else(|| AppError::not_found("Окно закрылось"))
}

/// Snap to a region of the window's current monitor work area.
/// `region`: left | right | top | bottom | top-left | top-right | bottom-left |
/// bottom-right | center | maximize
pub fn snap(hwnd: i64, region: &str) -> AppResult<WindowInfo> {
    let h = require(hwnd)?;
    if region == "maximize" {
        return maximize(hwnd);
    }
    let mon = display::monitor_for_window(h).ok_or_else(|| AppError::platform("Монитор окна не найден"))?;
    let w = mon.work_area;
    let (hw, hh) = (w.width / 2, w.height / 2);
    let rect = match region {
        "left" => Rect { x: w.x, y: w.y, width: hw, height: w.height },
        "right" => Rect { x: w.x + hw, y: w.y, width: w.width - hw, height: w.height },
        "top" => Rect { x: w.x, y: w.y, width: w.width, height: hh },
        "bottom" => Rect { x: w.x, y: w.y + hh, width: w.width, height: w.height - hh },
        "top-left" => Rect { x: w.x, y: w.y, width: hw, height: hh },
        "top-right" => Rect { x: w.x + hw, y: w.y, width: w.width - hw, height: hh },
        "bottom-left" => Rect { x: w.x, y: w.y + hh, width: hw, height: w.height - hh },
        "bottom-right" => Rect { x: w.x + hw, y: w.y + hh, width: w.width - hw, height: w.height - hh },
        "center" => {
            let cw = (w.width as f64 * 0.62) as i32;
            let ch = (w.height as f64 * 0.72) as i32;
            Rect { x: w.x + (w.width - cw) / 2, y: w.y + (w.height - ch) / 2, width: cw, height: ch }
        }
        other => return Err(AppError::invalid(format!("Неизвестная область размещения: {other}"))),
    };
    move_resize(hwnd, rect)
}

/// Find windows whose title or process name contains every word of `query`
/// (case-insensitive). Ordered by Z-order.
pub fn find_windows(query: &str) -> Vec<WindowInfo> {
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Vec::new();
    }
    list_windows()
        .into_iter()
        .filter(|w| {
            let hay = format!("{} {}", w.title, w.process).to_lowercase();
            words.iter().all(|word| hay.contains(word.as_str()))
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForegroundEntry {
    pub hwnd: i64,
    pub title: String,
    pub process: String,
    pub at: DateTime<Utc>,
}

/// Most-recent-first history of foreground application windows.
#[derive(Default)]
pub struct ForegroundHistory {
    entries: Mutex<VecDeque<ForegroundEntry>>,
}

impl ForegroundHistory {
    const MAX: usize = 16;

    pub fn record(&self, hwnd: HWND) {
        if !is_app_window(hwnd) {
            return;
        }
        let Some(info) = describe(hwnd) else { return };
        let mut e = self.entries.lock();
        e.retain(|x| x.hwnd != info.hwnd);
        e.push_front(ForegroundEntry { hwnd: info.hwnd, title: info.title, process: info.process, at: Utc::now() });
        e.truncate(Self::MAX);
    }

    /// Recent windows that still exist.
    pub fn recent(&self) -> Vec<ForegroundEntry> {
        let mut e = self.entries.lock();
        e.retain(|x| unsafe { IsWindow(Some(to_hwnd(x.hwnd))).as_bool() });
        e.iter().cloned().collect()
    }

    /// The window that was in the foreground before the current one.
    pub fn previous(&self) -> Option<ForegroundEntry> {
        let current = unsafe { GetForegroundWindow() }.0 as isize as i64;
        self.recent().into_iter().find(|x| x.hwnd != current)
    }
}
