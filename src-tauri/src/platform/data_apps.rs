use super::winmgr::WindowInfo;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub name: String,
    /// AppsFolder parsing name (AUMID for packaged apps, path-like otherwise).
    pub id: String,
    pub exe: Option<String>,
    /// "packaged" | "desktop"
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    pub app: AppEntry,
    /// A window belonging to the app was observed after launching.
    pub verified: bool,
    pub window: Option<WindowInfo>,
    pub note: Option<String>,
}
