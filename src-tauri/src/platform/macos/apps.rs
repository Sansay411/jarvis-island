pub use super::data_apps::*;
use crate::error::AppError;
use crate::error::AppResult;
use crate::platform::winmgr::WindowInfo;
use std::time::Duration;
#[derive(Default)]
pub struct AppIndex;
impl AppIndex {
    pub fn ensure_fresh(&self, _max_age: Duration) -> AppResult<()> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
    pub fn search(&self, _query: &str, _limit: usize) -> AppResult<Vec<AppEntry>> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
    pub fn search_scored(&self, _query: &str, _limit: usize) -> AppResult<Vec<(u32, AppEntry)>> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
    pub fn all(&self) -> AppResult<Vec<AppEntry>> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
    pub fn by_id(&self, _id: &str) -> AppResult<Option<AppEntry>> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
}
pub fn shell_open(_target: &str, _args: Option<&str>, _cwd: Option<&str>) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn launch(_app: &AppEntry, _timeout: Duration) -> AppResult<LaunchOutcome> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn running_windows(_app: &AppEntry) -> Vec<WindowInfo> {
    Vec::new()
}
