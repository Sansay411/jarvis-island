use super::display::Rect;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementInfo {
    pub id: usize,
    pub role: &'static str,
    pub name: String,
    pub value: Option<String>,
    pub rect: Rect,
    pub enabled: bool,
    pub focused: bool,
    pub actions: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub snapshot_id: u64,
    pub hwnd: i64,
    pub window_title: String,
    pub elements: Vec<ElementInfo>,
    pub total_found: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActOutcome {
    pub action: String,
    pub element: String,
    /// State read back after the action, where the pattern exposes one.
    pub after: Option<String>,
}
