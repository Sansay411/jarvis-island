//! Microsoft neural voices (Azure AI Speech, official REST API). The free F0
//! tier gives 500 000 characters a month — far more than JARVIS speaks —
//! with the natural Russian voices Svetlana, Dariya and Dmitry. Only the
//! reply text leaves the PC; recognition stays local. Without a network the
//! local voice speaks instead.
//!
//! API: POST https://{region}.tts.speech.microsoft.com/cognitiveservices/v1
//! with SSML; `raw-24khz-16bit-mono-pcm` is exactly the playback format.

use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::store::secrets;

/// Voice ids in settings: `azure:<ShortName>`.
pub const PREFIX: &str = "azure:";

/// Russian neural voices (ShortName, label), default first.
pub const VOICES: &[(&str, &str)] = &[
    ("ru-RU-DmitryNeural", "Дмитрий, мужской"),
    ("ru-RU-SvetlanaNeural", "Светлана, женский"),
    ("ru-RU-DariyaNeural", "Дарья, женский"),
];

pub fn credentials() -> Option<(String, String)> {
    let key = secrets::get(secrets::AZURE_SPEECH_KEY).ok().flatten().filter(|k| !k.trim().is_empty())?;
    let region = secrets::get(secrets::AZURE_SPEECH_REGION).ok().flatten().filter(|r| !r.trim().is_empty())?;
    Some((key.trim().to_string(), region.trim().to_lowercase()))
}

pub fn configured() -> bool {
    credentials().is_some()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

pub struct AzureVoice {
    pub voice: String,
    key: String,
    region: String,
    /// Speaking rate as SSML percent («+10%»).
    rate: String,
    http: reqwest::blocking::Client,
}

impl AzureVoice {
    pub fn new(voice: &str, rate: f64) -> AppResult<AzureVoice> {
        let (key, region) = credentials().ok_or_else(|| AppError::invalid("Для голоса Microsoft нужен ключ Azure Speech — Настройки → Голос"))?;
        if !region.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(AppError::invalid("Регион Azure — латиница без пробелов, например westeurope"));
        }
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(12))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| AppError::internal(e.to_string()))?;
        let pct = ((rate.clamp(0.5, 2.0) - 1.0) * 100.0).round() as i32;
        let v = AzureVoice { voice: voice.to_string(), key, region, rate: format!("{pct:+}%"), http };
        v.warm();
        Ok(v)
    }

    /// Open the TLS connection ahead of the first phrase (the voices list
    /// endpoint is cheap and counts no characters).
    fn warm(&self) {
        let url = format!("https://{}.tts.speech.microsoft.com/cognitiveservices/voices/list", self.region);
        let (http, key) = (self.http.clone(), self.key.clone());
        std::thread::spawn(move || {
            let _ = http.get(url).header("Ocp-Apim-Subscription-Key", key).send();
        });
    }

    /// PCM16 mono at 24 kHz.
    pub fn synthesize(&self, text: &str) -> AppResult<Vec<i16>> {
        let ssml = format!(
            "<speak version='1.0' xml:lang='ru-RU'><voice name='{}'><prosody rate='{}'>{}</prosody></voice></speak>",
            self.voice,
            self.rate,
            escape(text)
        );
        let url = format!("https://{}.tts.speech.microsoft.com/cognitiveservices/v1", self.region);
        let res = self
            .http
            .post(url)
            .header("Ocp-Apim-Subscription-Key", &self.key)
            .header("Content-Type", "application/ssml+xml")
            .header("X-Microsoft-OutputFormat", "raw-24khz-16bit-mono-pcm")
            .header("User-Agent", "JARVIS")
            .body(ssml)
            .send()
            .map_err(|e| AppError::platform(format!("Голос Microsoft недоступен: {e}")))?;
        let status = res.status();
        if !status.is_success() {
            return Err(AppError::platform(match status.as_u16() {
                401 => "Ключ Azure Speech не подошёл (или регион не тот)".to_string(),
                429 => "Лимит Azure Speech исчерпан — говорю локальным голосом".to_string(),
                c => format!("Голос Microsoft: ошибка {c}"),
            }));
        }
        let bytes = res.bytes().map_err(|e| AppError::platform(format!("Голос Microsoft: {e}")))?;
        Ok(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect())
    }
}
