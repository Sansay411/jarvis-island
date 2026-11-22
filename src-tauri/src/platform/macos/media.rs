pub use super::data_media::*;
use crate::error::AppError;
use crate::error::AppResult;
use tauri::AppHandle;
pub const EVT_MEDIA: &str = "media:changed";
pub const EVT_ARTWORK: &str = "media:artwork";
pub struct SessionManager;
pub struct MediaWatcher;
impl MediaWatcher {
    pub fn spawn(_app: AppHandle) -> AppResult<()> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
}
pub fn manager() -> AppResult<SessionManager> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn snapshot() -> AppResult<MediaState> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn artwork() -> AppResult<ArtworkEvent> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn control(_action: MediaAction) -> AppResult<ControlOutcome> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn seek(_position_ms: i64) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
