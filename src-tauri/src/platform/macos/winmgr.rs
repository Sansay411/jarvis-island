pub use super::data_winmgr::*;
use crate::error::AppError;
use crate::error::AppResult;
use crate::platform::display::Rect;
use crate::platform::HWND;
#[derive(Default)]
pub struct ForegroundHistory;
impl ForegroundHistory {
    pub fn record(&self, _hwnd: HWND) {}
    pub fn recent(&self) -> Vec<ForegroundEntry> {
        Vec::new()
    }
    pub fn previous(&self) -> Option<ForegroundEntry> {
        None
    }
}
pub fn to_hwnd(h: i64) -> HWND {
    HWND(h as isize)
}
pub fn describe(_hwnd: HWND) -> Option<WindowInfo> {
    None
}
pub fn list_windows() -> Vec<WindowInfo> {
    Vec::new()
}
pub fn foreground_window() -> Option<WindowInfo> {
    None
}
pub fn focus(_hwnd: i64) -> AppResult<WindowInfo> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn minimize(_hwnd: i64) -> AppResult<WindowInfo> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn maximize(_hwnd: i64) -> AppResult<WindowInfo> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn restore(_hwnd: i64) -> AppResult<WindowInfo> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn close(_hwnd: i64) -> AppResult<CloseOutcome> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn move_resize(_hwnd: i64, _rect: Rect) -> AppResult<WindowInfo> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn snap(_hwnd: i64, _region: &str) -> AppResult<WindowInfo> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn find_windows(_query: &str) -> Vec<WindowInfo> {
    Vec::new()
}
