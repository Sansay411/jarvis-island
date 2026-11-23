pub use super::data_brightness::*;
use crate::error::AppError;
use crate::error::AppResult;
pub fn list() -> AppResult<Vec<DisplayBrightness>> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn set(_percent: u8, _target: Option<&str>) -> AppResult<Vec<DisplayBrightness>> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
