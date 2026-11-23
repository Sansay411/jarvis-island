pub use super::data_browser::*;
use crate::error::AppError;
use crate::error::AppResult;
use crate::platform::winmgr::WindowInfo;
pub const BROWSERS: &[(&str, &str)] = &[
    ("chrome.exe", "Chrome"),
    ("msedge.exe", "Edge"),
    ("browser.exe", "Яндекс Браузер"),
    ("brave.exe", "Brave"),
    ("opera.exe", "Opera"),
    ("vivaldi.exe", "Vivaldi"),
    ("firefox.exe", "Firefox"),
];
pub fn browser_label(_process: &str) -> Option<&'static str> {
    None
}
pub fn browser_windows(_which: Option<&str>) -> Vec<WindowInfo> {
    Vec::new()
}
pub fn state(_hwnd: i64) -> AppResult<BrowserState> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn switch_tab(
    _hwnd: i64,
    _query: Option<&str>,
    _index: Option<usize>,
) -> AppResult<BrowserState> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn close_tab(
    _hwnd: i64,
    _query: Option<&str>,
    _index: Option<usize>,
) -> AppResult<(String, bool, BrowserState)> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn page_text(_hwnd: i64, _max_chars: usize) -> AppResult<String> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
