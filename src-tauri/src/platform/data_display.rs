use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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

/// How the foreground window relates to the island's monitor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ForegroundShape {
    /// Covers the whole island monitor (games, video, presentations).
    pub fullscreen: bool,
    /// Touches the top of the work area under the island (maximised or
    /// snapped windows whose title bar sits where the island hangs).
    pub top_covered: bool,
}
