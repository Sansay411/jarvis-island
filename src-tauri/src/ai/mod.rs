//! AI provider layer. Today every supported provider speaks the
//! OpenAI-compatible Chat Completions protocol; the agent depends only on
//! [`types`], so adding a provider with a different wire format means a new
//! adapter next to [`openai`].

pub mod openai;
pub mod probe;
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

/// Whether `base_url` points to this machine or a `.local` mDNS host (where
/// OpenAI-compatible servers commonly run without an API key).
pub fn is_local_url(base_url: &str) -> bool {
    url::Url::parse(base_url.trim()).ok().is_some_and(|u| {
        matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "::1" | "[::1]"))
            || u.host_str().is_some_and(|h| h.ends_with(".local"))
    })
}

/// Resolve the optional API key for `base_url`, binding it to `KEY_ORIGIN` so
/// a saved key never leaks to a different origin.
pub fn resolve_key_for_url(preset_id: &str, base_url: &str, store: &Store, require_for_remote: bool) -> AppResult<Option<String>> {
    let preset = settings::preset(preset_id);
    if !preset.needs_key {
        return Ok(None);
    }
    let key = secrets::get(secrets::LLM_API_KEY)?.filter(|k| !k.trim().is_empty());
    let origin = origin_of(base_url);
    match (key, store.kv_get(KEY_ORIGIN).filter(|o| !o.is_empty())) {
        (None, _) if preset.id == "custom" && is_local_url(base_url) => Ok(None),
        (None, _) if !require_for_remote => Ok(None),
        (None, _) => Err(AppError::invalid("Добавьте API-ключ провайдера в настройках")),
        (Some(k), Some(owner)) if owner == origin => Ok(Some(k)),
        (Some(_), Some(_owner)) if !require_for_remote => Ok(None),
        (Some(_), Some(owner)) => Err(AppError::invalid(format!(
            "Сохранённый ключ относится к {owner}. Добавьте ключ для {origin} в настройках"
        ))),
        (Some(k), None) => {
            store.kv_set(KEY_ORIGIN, &origin)?;
            Ok(Some(k))
        }
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
    if preset.id == "deepseek" && s.provider.base_url.trim().is_empty() {
        let key = secrets::get(secrets::LLM_API_KEY)?.filter(|k| !k.trim().is_empty());
        if key.is_none() {
            // DeepSeek without an own key (installs from before Jarvis
            // Cloud): the same model through the free tier.
            return Ok(OpenAiCompatible::new(
                crate::cloud::API.into(),
                None,
                settings::preset("jarvis-cloud").default_model.into(),
                false,
                s.provider.temperature,
            ));
        }
    }
    let key = resolve_key_for_url(&s.provider.preset, &base_url, store, true)?;
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
