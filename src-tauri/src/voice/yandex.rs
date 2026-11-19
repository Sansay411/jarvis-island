//! Yandex SpeechKit voices (API v3, REST), streamed: sound plays while the
//! rest of the phrase is still being made. Native Russian voices with
//! correct stress and intonation; ≈1 ₸ per request of up to 250 characters
//! (the KZ starter grant covers tens of thousands of replies). Only the reply
//! text leaves the PC; without a network the local voice speaks instead.
//!
//! API: POST https://tts.api.ml.yandexcloud.kz/tts/v3/utteranceSynthesis
//! (Kazakhstan installation — only v3 works there; accounts of the Russian
//! one answer at tts.api.cloud.yandex.net) with `Authorization: Api-Key …`
//! of a service account that has the role `ai.speechkit-tts.user`. The
//! answer is a stream of JSON objects `{"result":{"audioChunk":{"data":…}}}`
//! with base64 LINEAR16 PCM at the rate asked for.

use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine;
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::store::secrets;

/// Voice ids in settings: `yandex:<voice>` or `yandex:<voice>/<role>`.
pub const PREFIX: &str = "yandex:";

/// Russian voices (id, label), default first. Roles are the voice's own
/// manners from the SpeechKit list (good = доброжелательный).
pub const VOICES: &[(&str, &str)] = &[
    ("kirill", "Кирилл, мужской"),
    ("kirill/good", "Кирилл, доброжелательный"),
    ("kirill/strict", "Кирилл, строгий"),
    ("alexander", "Александр, мужской"),
    ("alexander/good", "Александр, доброжелательный"),
    ("anton", "Антон, мужской"),
    ("ermil", "Ермил, мужской"),
    ("zahar", "Захар, мужской"),
    ("filipp", "Филипп, мужской"),
    ("marina/friendly", "Марина, дружелюбная"),
    ("alena/good", "Алёна, доброжелательная"),
    ("dasha/friendly", "Даша, дружелюбная"),
    ("masha/good", "Маша, доброжелательная"),
    ("lera/friendly", "Лера, дружелюбная"),
    ("jane", "Джейн, женский"),
    ("julia", "Юлия, женский"),
];

pub const DEFAULT_VOICE: &str = "kirill";

/// Kazakhstan installation first (the user's console is kz), then Russia.
const HOSTS: &[&str] = &["https://tts.api.ml.yandexcloud.kz", "https://tts.api.cloud.yandex.net"];

const RATE: u32 = 24_000;

pub fn key() -> Option<String> {
    secrets::get(secrets::YANDEX_SPEECH_KEY).ok().flatten().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

pub fn configured() -> bool {
    key().is_some()
}

/// Until when SpeechKit is not asked (bad key, no role, rate limit).
static REFUSED_UNTIL: Mutex<Option<std::time::Instant>> = Mutex::new(None);
/// The host that answered last (index into HOSTS).
static HOST: Mutex<usize> = Mutex::new(0);

pub fn label(id: &str) -> String {
    VOICES.iter().find(|(v, _)| *v == id).map(|(_, l)| l.to_string()).unwrap_or_else(|| id.to_string())
}

/// `kirill/good` → ("kirill", Some("good")); None for anything that is not
/// plain latin (the id goes into a JSON body, but keep it tidy).
fn split(id: &str) -> Option<(String, Option<String>)> {
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    match id.split_once('/') {
        Some((v, r)) if ok(v) && ok(r) => Some((v.into(), Some(r.into()))),
        None if ok(id) => Some((id.into(), None)),
        _ => None,
    }
}

/// The request body (pure, for tests).
pub fn body(text: &str, voice: &str, role: Option<&str>, speed: f64) -> serde_json::Value {
    let mut hints = vec![serde_json::json!({ "voice": voice })];
    if let Some(r) = role {
        hints.push(serde_json::json!({ "role": r }));
    }
    if (speed - 1.0).abs() > 0.01 {
        hints.push(serde_json::json!({ "speed": speed }));
    }
    serde_json::json!({
        "text": text,
        "hints": hints,
        "outputAudioSpec": { "rawAudio": { "audioEncoding": "LINEAR16_PCM", "sampleRateHertz": RATE } },
        "loudnessNormalizationType": "LUFS",
        // Over 250 characters a request needs unsafe mode (up to 5000).
        "unsafeMode": text.chars().count() > 240,
    })
}

/// One streamed message: Ok(Some(pcm)) audio, Ok(None) something else
/// (text templates, lengths), Err an error object inside the stream.
pub fn chunk(msg: &serde_json::Value) -> AppResult<Option<Vec<i16>>> {
    if let Some(err) = msg.get("error") {
        let text = err["message"].as_str().unwrap_or("ошибка");
        return Err(AppError::platform(format!("Голос Яндекса: {text}")));
    }
    let Some(data) = msg["result"]["audioChunk"]["data"].as_str() else { return Ok(None) };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| AppError::platform(format!("Голос Яндекса: {e}")))?;
    Ok(Some(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()))
}

pub struct YandexVoice {
    pub id: String,
    pub name: String,
    voice: String,
    role: Option<String>,
    key: String,
    speed: f64,
    http: reqwest::blocking::Client,
}

impl YandexVoice {
    pub fn new(id: &str, rate: f64) -> AppResult<YandexVoice> {
        let key = key().ok_or_else(|| AppError::invalid("Для голоса Яндекса нужен API-ключ SpeechKit — Настройки → Голос"))?;
        let (voice, role) = split(id).ok_or_else(|| AppError::invalid("Неверный голос Яндекса"))?;
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(20))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| AppError::internal(e.to_string()))?;
        let v = YandexVoice { id: id.to_string(), name: label(id), voice, role, key, speed: rate.clamp(0.7, 1.4), http };
        v.warm();
        Ok(v)
    }

    /// Open the TLS connection ahead of the first phrase (no request is billed).
    fn warm(&self) {
        let http = self.http.clone();
        let host = HOSTS[*HOST.lock()];
        std::thread::spawn(move || {
            let _ = http.head(host).send();
        });
    }

    /// PCM16 mono at 24 kHz, handed to `sink` as it arrives; `sink` returns
    /// false to stop (the phrase was hushed).
    pub fn stream(&self, text: &str, sink: &mut dyn FnMut(&[i16]) -> bool) -> AppResult<()> {
        static WARNED: OnceLock<()> = OnceLock::new();
        if let Some(until) = *REFUSED_UNTIL.lock() {
            if std::time::Instant::now() < until {
                return Err(AppError::platform("Яндекс отказал недавно — говорю локальным голосом"));
            }
        }
        let body = body(text, &self.voice, self.role.as_deref(), self.speed);
        let first = *HOST.lock();
        let mut last_err = None;
        // The answering host first; on a refused key or no connection try the
        // other installation once (a key of the Russian cloud, for example).
        for i in [first, 1 - first] {
            let res = match self
                .http
                .post(format!("{}/tts/v3/utteranceSynthesis", HOSTS[i]))
                .header("Authorization", format!("Api-Key {}", self.key))
                .json(&body)
                .send()
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(AppError::platform(format!("Голос Яндекса недоступен: {e}")));
                    continue;
                }
            };
            let status = res.status();
            if !status.is_success() {
                let detail = res.text().unwrap_or_default();
                if WARNED.set(()).is_ok() {
                    tracing::warn!(status = status.as_u16(), host = HOSTS[i], %detail, "Yandex SpeechKit refused");
                }
                let code = status.as_u16();
                last_err = Some(AppError::platform(match code {
                    401 => "Ключ Яндекса не подошёл".to_string(),
                    403 => "У сервисного аккаунта Яндекса нет роли ai.speechkit-tts.user".to_string(),
                    429 => "Яндекс: слишком много запросов — говорю локальным голосом".to_string(),
                    c => format!("Голос Яндекса: ошибка {c}"),
                }));
                if matches!(code, 401 | 403) {
                    continue;
                }
                if code == 429 {
                    *REFUSED_UNTIL.lock() = Some(std::time::Instant::now() + Duration::from_secs(60));
                }
                return Err(last_err.unwrap());
            }
            *HOST.lock() = i;
            let mut pending: Vec<i16> = Vec::with_capacity(2400);
            for msg in serde_json::Deserializer::from_reader(res).into_iter::<serde_json::Value>() {
                let msg = msg.map_err(|e| AppError::platform(format!("Голос Яндекса: {e}")))?;
                if let Some(pcm) = chunk(&msg)? {
                    pending.extend_from_slice(&pcm);
                    if pending.len() >= 1200 {
                        if !sink(&pending) {
                            return Ok(());
                        }
                        pending.clear();
                    }
                }
            }
            if !pending.is_empty() {
                sink(&pending);
            }
            return Ok(());
        }
        // Both installations refused the key: do not ask again for a while.
        if let Some(AppError::Platform(m)) = &last_err {
            if m.contains("Ключ") || m.contains("роли") {
                *REFUSED_UNTIL.lock() = Some(std::time::Instant::now() + Duration::from_secs(600));
            }
        }
        Err(last_err.unwrap_or_else(|| AppError::platform("Голос Яндекса недоступен")))
    }
}

/// A new key may fix what the old one could not.
pub fn forget_refusal() {
    *REFUSED_UNTIL.lock() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_ids_are_split_into_voice_and_role() {
        assert_eq!(split("kirill"), Some(("kirill".into(), None)));
        assert_eq!(split("kirill/good"), Some(("kirill".into(), Some("good".into()))));
        assert_eq!(split("Kirill\"}"), None);
        assert_eq!(split("a/"), None);
        assert!(VOICES.iter().all(|(id, _)| split(id).is_some()));
    }

    #[test]
    fn request_asks_for_raw_24k_pcm_and_unsafe_mode_only_for_long_text() {
        let b = body("Привет", "kirill", Some("good"), 1.0);
        assert_eq!(b["outputAudioSpec"]["rawAudio"]["audioEncoding"], "LINEAR16_PCM");
        assert_eq!(b["outputAudioSpec"]["rawAudio"]["sampleRateHertz"], 24000);
        assert_eq!(b["hints"][0]["voice"], "kirill");
        assert_eq!(b["hints"][1]["role"], "good");
        assert_eq!(b["hints"].as_array().unwrap().len(), 2);
        assert_eq!(b["unsafeMode"], false);
        let long = "слово ".repeat(60);
        assert_eq!(body(&long, "kirill", None, 1.2)["unsafeMode"], true);
        assert_eq!(body(&long, "kirill", None, 1.2)["hints"][1]["speed"], 1.2);
    }

    #[test]
    fn streamed_chunks_are_decoded_and_errors_surface() {
        let pcm: Vec<u8> = [1i16, -2, 300].iter().flat_map(|s| s.to_le_bytes()).collect();
        let data = base64::engine::general_purpose::STANDARD.encode(pcm);
        let msg = serde_json::json!({ "result": { "audioChunk": { "data": data } } });
        assert_eq!(chunk(&msg).unwrap(), Some(vec![1, -2, 300]));
        assert_eq!(chunk(&serde_json::json!({ "result": { "textChunk": { "text": "x" } } })).unwrap(), None);
        assert!(chunk(&serde_json::json!({ "error": { "grpcCode": 3, "message": "bad voice" } })).is_err());
        // A stream of objects back to back, as the gateway sends them.
        let stream = format!("{}\n{}", msg, serde_json::json!({ "result": { "audioChunk": { "data": "AAA=" } } }));
        let n = serde_json::Deserializer::from_str(&stream).into_iter::<serde_json::Value>().count();
        assert_eq!(n, 2);
    }

    /// Live check with the key from Windows Credential Manager (outside the
    /// sandbox): one short phrase, how long until the first sound.
    #[test]
    #[ignore]
    fn live_yandex_sample() {
        let v = YandexVoice::new(DEFAULT_VOICE, 1.0).expect("key");
        let started = std::time::Instant::now();
        let mut first = None;
        let mut total = 0usize;
        let res = v.stream("Добрый вечер. Всё готово, можно работать.", &mut |pcm| {
            first.get_or_insert(started.elapsed());
            total += pcm.len();
            true
        });
        println!("result={res:?} first_sound={first:?} seconds={:.2} host={}", total as f64 / RATE as f64, HOSTS[*HOST.lock()]);
        assert!(res.is_ok() && total > RATE as usize / 2);
    }
}
