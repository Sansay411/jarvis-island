use super::tts::TtsVoice;
use crate::error::{AppError, AppResult};
pub fn windows_voices() -> AppResult<Vec<TtsVoice>> {
    Ok(Vec::new())
}
pub struct WindowsSpeaker {
    pub voice: String,
}
impl WindowsSpeaker {
    pub fn new(_voice_id: &str, _rate: f64) -> AppResult<Self> {
        Err(AppError::unsupported(
            "Синтез речи пока не поддерживается на macOS",
        ))
    }
    pub fn synthesize_raw(&self, _text: &str) -> AppResult<(u32, Vec<f32>)> {
        Err(AppError::unsupported(
            "Синтез речи пока не поддерживается на macOS",
        ))
    }
}
