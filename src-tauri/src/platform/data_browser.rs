use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub index: usize,
    pub title: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub browser: String,
    pub minimized: bool,
    pub hwnd: i64,
    pub window_title: String,
    pub url: Option<String>,
    pub tabs: Vec<Tab>,
}
