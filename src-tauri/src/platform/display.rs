//! Monitor geometry, DPI and fullscreen detection.

use serde::Serialize;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR,
    MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect, MONITORINFOF_PRIMARY};

use super::from_wide;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Rect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: isize,
    pub name: String,
    pub primary: bool,
    /// Full monitor bounds in physical pixels (virtual-screen coordinates).
    pub bounds: Rect,
    /// Bounds minus taskbar/app bars.
    pub work_area: Rect,
    /// Effective DPI scale (1.0 = 96 DPI).
    pub scale: f64,
}

fn info_for(hmon: HMONITOR) -> Option<MonitorInfo> {
    unsafe {
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(hmon, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        Some(MonitorInfo {
            id: hmon.0 as isize,
            name: from_wide(&mi.szDevice),
            primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            bounds: mi.monitorInfo.rcMonitor.into(),
            work_area: mi.monitorInfo.rcWork.into(),
            scale: dx as f64 / 96.0,
        })
    }
}

pub fn primary_monitor() -> Option<MonitorInfo> {
    unsafe { info_for(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)) }
}

pub fn monitor_for_window(hwnd: HWND) -> Option<MonitorInfo> {
    unsafe { info_for(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)) }
}

pub fn monitor_for_point(x: i32, y: i32) -> Option<MonitorInfo> {
    unsafe { info_for(MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST)) }
}

pub fn list_monitors() -> Vec<MonitorInfo> {
    unsafe extern "system" fn cb(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<MonitorInfo>) };
        if let Some(info) = info_for(hmon) {
            out.push(info);
        }
        true.into()
    }
    let mut out: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// True when the foreground window covers its whole monitor (games, video
/// players, presentations), or the shell reports a fullscreen/presentation
/// state. The desktop and the shell itself never count.
pub fn foreground_is_fullscreen(own_hwnd: isize) -> bool {
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE {
                return true;
            }
            // QUNS_BUSY ("a fullscreen app is running") is also reported for
            // some borderless windows, so it is confirmed by the rect check below.
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg.0 as isize == own_hwnd {
            return false;
        }
        let class = class_name(fg);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let Some(mon) = monitor_for_window(fg) else { return false };
        let b = mon.bounds;
        r.left <= b.x && r.top <= b.y && r.right >= b.x + b.width && r.bottom >= b.y + b.height
    }
}
