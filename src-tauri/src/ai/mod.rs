//! AI provider layer. Today every supported provider speaks the
//! OpenAI-compatible Chat Completions protocol; the agent depends only on
//! [`types`], so adding a provider with a different wire format means a new
//! adapter next to [`openai`].

pub mod openai;
pub mod types;

use crate::error::{AppError, AppResult};
use crate::store::{secrets, settings::{self, Settings}};

pub use openai::OpenAiCompatible;

/// Build the configured client, or explain what is missing.
pub fn client_from_settings(s: &Settings) -> AppResult<OpenAiCompatible> {
    let preset = settings::preset(&s.provider.preset);
    let base_url = s.provider.effective_base_url();
    if base_url.is_empty() {
        return Err(AppError::invalid("Укажите адрес API провайдера в настройках"));
    }
    if s.provider.model.trim().is_empty() {
        return Err(AppError::invalid("Укажите модель в настройках"));
    }
    let key = secrets::get(secrets::LLM_API_KEY)?.filter(|k| !k.trim().is_empty());
    if preset.needs_key && key.is_none() {
        return Err(AppError::invalid("Добавьте API-ключ провайдера в настройках"));
    }
    Ok(OpenAiCompatible::new(base_url, key, s.provider.model.trim().to_string(), preset.dashscope, s.provider.temperature))
}
