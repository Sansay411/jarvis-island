use crate::error::{AppError, AppResult};
pub const LLM_API_KEY: &str = "JARVIS/llm-api-key";
pub const VOICE_API_KEY: &str = "JARVIS/voice-api-key";
/// Microsoft Azure Speech (free F0 tier): key and its region («westeurope»).
pub const AZURE_SPEECH_KEY: &str = "JARVIS/azure-speech-key";
pub const AZURE_SPEECH_REGION: &str = "JARVIS/azure-speech-region";
/// ElevenLabs API key (the user's own plan).
pub const ELEVENLABS_KEY: &str = "JARVIS/elevenlabs-key";
/// Yandex SpeechKit: API key of a service account (role ai.speechkit-tts.user).
pub const YANDEX_SPEECH_KEY: &str = "JARVIS/yandex-speech-key";
/// Google Calendar OAuth client secret (the user's own Cloud project).
pub const GOOGLE_CLIENT_SECRET: &str = "JARVIS/google-client-secret";
pub const GOOGLE_REFRESH_TOKEN: &str = "JARVIS/google-refresh-token";
/// Telegram bot token from @BotFather.
pub const TELEGRAM_BOT_TOKEN: &str = "JARVIS/telegram-bot-token";
/// Jarvis Cloud device token (signed by the service, not a provider key).
pub const CLOUD_TOKEN: &str = "JARVIS/cloud-token";

fn entry(target: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(target, "jarvis")
        .map_err(|_| AppError::Storage("Не удалось открыть Связку ключей macOS".into()))
}
pub fn set(target: &str, secret: &str) -> AppResult<()> {
    entry(target)?
        .set_password(secret)
        .map_err(|_| AppError::Storage("Не удалось сохранить ключ в Связке ключей macOS".into()))
}
pub fn get(target: &str) -> AppResult<Option<String>> {
    match entry(target)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(AppError::Storage(
            "Не удалось прочитать ключ из Связки ключей macOS".into(),
        )),
    }
}
pub fn exists(target: &str) -> bool {
    matches!(get(target), Ok(Some(s)) if !s.is_empty())
}
pub fn delete(target: &str) -> AppResult<()> {
    match entry(target)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(AppError::Storage(
            "Не удалось удалить ключ из Связки ключей macOS".into(),
        )),
    }
}
