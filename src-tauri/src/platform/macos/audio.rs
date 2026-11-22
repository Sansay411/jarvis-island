pub use super::data_audio::*;
use crate::error::AppError;
use crate::error::AppResult;
pub fn get_state() -> AppResult<AudioState> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn set_volume(_percent: u8) -> AppResult<AudioState> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn adjust_volume(_delta: i16) -> AppResult<AudioState> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn set_mute(_muted: bool) -> AppResult<AudioState> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn list_output_devices() -> AppResult<Vec<OutputDevice>> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn list_app_sessions() -> AppResult<Vec<AppAudioSession>> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn set_app_volume(_process: &str, _percent: u8) -> AppResult<Vec<AppAudioSession>> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
