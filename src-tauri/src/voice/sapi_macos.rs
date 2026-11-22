use crate::error::{AppError, AppResult};
pub const PREFIX: &str = "sapi:";
pub const RATE: u32 = 24_000;
pub struct SapiVoiceInfo {
    pub id: String,
    pub name: String,
    pub languages: String,
}
impl SapiVoiceInfo {
    pub fn russian(&self) -> bool {
        false
    }
}
pub fn voices() -> Vec<SapiVoiceInfo> {
    Vec::new()
}
pub struct SapiVoice {
    pub name: String,
}
impl SapiVoice {
    pub fn new(_token_id: &str, _rate: f64) -> AppResult<Self> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
    pub fn synthesize(&self, _text: &str) -> AppResult<Vec<i16>> {
        Err(AppError::unsupported("Пока не поддерживается на macOS"))
    }
}
