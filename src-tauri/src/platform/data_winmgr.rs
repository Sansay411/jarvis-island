use super::display::Rect;
use chrono::{DateTime, Utc};
use serde::Serialize;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseOutcome {
    pub closed: bool,
    /// The app is still open — usually because it asked about unsaved work.
    pub still_open_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForegroundEntry {
    pub hwnd: i64,
    pub title: String,
    pub process: String,
    pub at: DateTime<Utc>,
}
