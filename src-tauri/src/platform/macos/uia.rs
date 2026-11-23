pub use super::data_uia::*;
use crate::error::AppError;
use crate::error::AppResult;
use crate::platform::HWND;
pub fn snapshot(_hwnd: HWND, _budget: usize) -> AppResult<Snapshot> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn render(_s: &Snapshot) -> String {
    String::new()
}
pub fn names_at_point(_x: i32, _y: i32) -> Option<Vec<String>> {
    None
}
pub fn element_name(_snapshot_id: u64, _id: usize) -> Option<String> {
    None
}
pub fn act(
    _snapshot_id: u64,
    _id: usize,
    _action: &str,
    _value: Option<&str>,
) -> AppResult<ActOutcome> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
