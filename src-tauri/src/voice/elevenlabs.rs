//! ElevenLabs voices (official REST API), streamed: the first 50 ms of a
//! phrase play while the rest is still being made. v4 Turbo sounds clearly
//! better than Flash v2.5 at the same price (half a credit per character,
//! so the Starter plan's ~38 000 credits last for ~75 000 characters of
//! speech) and, streamed, starts as fast: ≈0.2 s to the first sound over a
//! kept-alive connection for both (measured).
//! Only the reply text leaves the PC; without a network the local voice
//! speaks instead.
//!
//! API: POST https://api.elevenlabs.io/v1/text-to-speech/{voice}/stream
//! with `output_format=pcm_24000` — exactly the playback format.

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::store::secrets;

/// Voice ids in settings: `eleven:<voice_id>`.
pub const PREFIX: &str = "eleven:";

pub const MODEL: &str = "eleven_v4_turbo";

/// Artem Lebedev from the ElevenLabs library: a rich, calm, commanding
/// Russian male voice made for conversation, verified for Russian on v4
/// Turbo — the closest to JARVIS among the most used Russian voices.
pub const DEFAULT_VOICE: &str = "rQOBu7YxCDxGiFdTm28w";

const API: &str = "https://api.elevenlabs.io";

pub fn key() -> Option<String> {
    secrets::get(secrets::ELEVENLABS_KEY).ok().flatten().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

pub fn configured() -> bool {
    key().is_some()
}

fn client(timeout: Duration) -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(timeout)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| AppError::internal(e.to_string()))
}

/// The account's voices (id, name), fetched once per run.
pub fn voices() -> Vec<(String, String)> {
    static CACHE: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);
    if let Some(v) = CACHE.lock().clone() {
        return v;
    }
    let Some(key) = key() else { return vec![] };
    let fetched = (|| -> Option<Vec<(String, String)>> {
        let res = client(Duration::from_secs(5)).ok()?.get(format!("{API}/v1/voices")).header("xi-api-key", key).send().ok()?;
        let body: serde_json::Value = res.error_for_status().ok()?.json().ok()?;
        let mut out: Vec<(String, String)> = body["voices"]
            .as_array()?
            .iter()
            .filter_map(|v| {
                let id = v["voice_id"].as_str()?.to_string();
                // «Brian - Deep, Resonant…» → «Brian».
                let name = v["name"].as_str()?.split(" - ").next()?.trim().to_string();
                Some((id, name))
            })
            .collect();
        // The default first; then the account's own and library voices
        // (Russian speakers live there) before the English premade ones.
        let rank = |id: &str| if id == DEFAULT_VOICE { 0 } else { 1 };
        out.sort_by_key(|(id, _)| rank(id));
        Some(out)
    })();
    match fetched {
        Some(v) => {
            *CACHE.lock() = Some(v.clone());
            v
        }
        None => vec![(DEFAULT_VOICE.to_string(), "Artem Lebedev".to_string())],
    }
}

pub struct ElevenVoice {
    pub voice: String,
    pub name: String,
    key: String,
    speed: f64,
    http: reqwest::blocking::Client,
}

impl ElevenVoice {
    pub fn new(voice: &str, rate: f64) -> AppResult<ElevenVoice> {
        let key = key().ok_or_else(|| AppError::invalid("Для голоса ElevenLabs нужен ключ — Настройки → Голос"))?;
        if !voice.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(AppError::invalid("Неверный голос ElevenLabs"));
        }
        let name = voices().into_iter().find(|(id, _)| id == voice).map(|(_, n)| n).unwrap_or_else(|| voice.to_string());
        let v = ElevenVoice { voice: voice.to_string(), name, key, speed: rate.clamp(0.7, 1.2), http: client(Duration::from_secs(15))? };
        v.warm();
        Ok(v)
    }

    /// Open the TLS connection ahead of the first phrase (costs no credits).
    fn warm(&self) {
        let (http, key) = (self.http.clone(), self.key.clone());
        std::thread::spawn(move || {
            let _ = http.get(format!("{API}/v1/models")).header("xi-api-key", key).send();
        });
    }

    /// PCM16 mono at 24 kHz, handed to `sink` as it arrives; `sink`
    /// returns false to stop (the phrase was hushed).
    pub fn stream(&self, text: &str, sink: &mut dyn FnMut(&[i16]) -> bool) -> AppResult<()> {
        static WARNED: OnceLock<()> = OnceLock::new();
        let body = serde_json::json!({
            "text": text,
            "model_id": MODEL,
            "language_code": "ru",
            "voice_settings": { "speed": self.speed },
        });
        let res = self
            .http
            .post(format!("{API}/v1/text-to-speech/{}/stream?output_format=pcm_24000", self.voice))
            .header("xi-api-key", &self.key)
            .json(&body)
            .send()
            .map_err(|e| AppError::platform(format!("Голос ElevenLabs недоступен: {e}")))?;
        let status = res.status();
        if !status.is_success() {
            let detail = res.text().unwrap_or_default();
            if WARNED.set(()).is_ok() {
                tracing::warn!(status = status.as_u16(), %detail, "ElevenLabs refused");
            }
            return Err(AppError::platform(match status.as_u16() {
                401 if detail.contains("quota") => "Кредиты ElevenLabs закончились — говорю локальным голосом".to_string(),
                401 => "Ключ ElevenLabs не подошёл".to_string(),
                429 => "ElevenLabs: слишком много запросов — говорю локальным голосом".to_string(),
                c => format!("Голос ElevenLabs: ошибка {c}"),
            }));
        }
        let mut reader = res;
        let mut buf = vec![0u8; 4800];
        let mut carry: Option<u8> = None;
        let mut samples = Vec::with_capacity(2400);
        loop {
            let n = reader.read(&mut buf).map_err(|e| AppError::platform(format!("Голос ElevenLabs: {e}")))?;
            if n == 0 {
                break;
            }
            let mut bytes = &buf[..n];
            if let Some(lo) = carry.take() {
                samples.push(i16::from_le_bytes([lo, bytes[0]]));
                bytes = &bytes[1..];
            }
            let pairs = bytes.chunks_exact(2);
            carry = pairs.remainder().first().copied();
            samples.extend(pairs.map(|b| i16::from_le_bytes([b[0], b[1]])));
            // Hand over ~50 ms at a time: small enough to start at once,
            // large enough not to flood the event loop.
            if samples.len() >= 1200 {
                if !sink(&samples) {
                    return Ok(());
                }
                samples.clear();
            }
        }
        if !samples.is_empty() {
            sink(&samples);
        }
        Ok(())
    }
}
