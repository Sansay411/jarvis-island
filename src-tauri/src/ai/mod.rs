//! AI provider layer. Today every supported provider speaks the
//! OpenAI-compatible Chat Completions protocol; the agent depends only on
//! [`types`], so adding a provider with a different wire format means a new
//! adapter next to [`openai`].

pub mod openai;
pub mod types;

use crate::error::{AppError, AppResult};
use crate::store::{secrets, settings::{self, Settings}, Store};

pub use openai::OpenAiCompatible;

/// Where the LLM key may be sent: the origin it was saved for.
pub const KEY_ORIGIN: &str = "llm_key_origin";

/// `scheme://host[:port]` of an API base URL.
pub fn origin_of(base_url: &str) -> String {
    match url::Url::parse(base_url.trim()) {
        Ok(u) => u.origin().ascii_serialization(),
        Err(_) => base_url.trim().trim_end_matches('/').to_lowercase(),
    }
}

/// Build the configured client, or explain what is missing.
///
/// The API key is sent only to the origin it was saved for and never to
/// presets that need no key, so switching providers (or pointing at a local
/// server) can't leak it elsewhere.
pub fn client_from_settings(s: &Settings, store: &Store) -> AppResult<OpenAiCompatible> {
    let preset = settings::preset(&s.provider.preset);
    let base_url = s.provider.effective_base_url();
    if base_url.is_empty() {
        return Err(AppError::invalid("Укажите адрес API провайдера в настройках"));
    }
    if s.provider.model.trim().is_empty() {
        return Err(AppError::invalid("Укажите модель в настройках"));
    }
    let key = if preset.needs_key {
        let key = secrets::get(secrets::LLM_API_KEY)?.filter(|k| !k.trim().is_empty());
        let origin = origin_of(&base_url);
        match (key, store.kv_get(KEY_ORIGIN).filter(|o| !o.is_empty())) {
            (None, _) => return Err(AppError::invalid("Добавьте API-ключ провайдера в настройках")),
            (Some(k), Some(owner)) if owner == origin => Some(k),
            (Some(_), Some(owner)) => {
                return Err(AppError::invalid(format!(
                    "Сохранённый ключ относится к {owner}. Добавьте ключ для {origin} в настройках"
                )))
            }
            // A key saved before keys were bound to an origin belongs to the
            // provider it has been used with.
            (Some(k), None) => {
                store.kv_set(KEY_ORIGIN, &origin)?;
                Some(k)
            }
        }
    } else {
        None
    };
    Ok(OpenAiCompatible::new(base_url, key, s.provider.model.trim().to_string(), preset.dashscope, s.provider.temperature))
}

#[cfg(test)]
mod tests {
    use super::origin_of;

    #[test]
    fn origins() {
        assert_eq!(origin_of("https://api.deepseek.com/v1"), "https://api.deepseek.com");
        assert_eq!(origin_of("https://API.deepseek.com/v1/"), "https://api.deepseek.com");
        assert_eq!(origin_of("http://localhost:11434/v1"), "http://localhost:11434");
        assert_ne!(origin_of("https://api.openai.com/v1"), origin_of("https://api.deepseek.com/v1"));
    }
}
