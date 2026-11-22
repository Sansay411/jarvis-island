pub use super::data_input::*;
use crate::error::AppError;
use crate::error::AppResult;
pub fn move_to(_x: i32, _y: i32) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn click(_x: i32, _y: i32, _button: Button, _count: u32) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn scroll(_x: i32, _y: i32, _dy_notches: i32, _dx_notches: i32) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn type_text(_text: &str) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn key_chord(_chord: &str) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
