pub use super::data_clipboard::*;
use crate::error::AppError;
use crate::error::AppResult;
pub fn read() -> AppResult<ClipboardContent> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn snapshot() -> AppResult<Snapshot> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn write_image(_png: &[u8], _dib: &[u8]) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn write_files(_paths: &[String]) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn restore(_prev: &ClipboardContent) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn write_text(_text: &str) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
