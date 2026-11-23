//! Jarvis Cloud: the free tier a fresh install starts on. DeepSeek and
//! Yandex SpeechKit sit behind a Cloudflare Worker (`cloud/` in the repo)
//! that holds the keys; the app never sees them. Each install registers
//! once and keeps a signed device token in the credential store. The Worker
//! caps every device and the whole service per day and answers 429 with
//! `error.code = "jarvis_limit"` when a cap is reached.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::error::{AppError, AppResult};
use crate::store::secrets;
use crate::voice::stt::{self, Transcript};

pub const BASE: &str = "https://jarvis-cloud.credo-x5.workers.dev";
/// OpenAI-compatible base URL of the chat model.
pub const API: &str = "https://jarvis-cloud.credo-x5.workers.dev/v1";

const UA: &str = concat!("JARVIS/", env!("CARGO_PKG_VERSION"));

/// Whether a provider URL points at Jarvis Cloud.
pub fn is_cloud(base_url: &str) -> bool {
    crate::ai::origin_of(base_url) == BASE
}

static TOKEN: Mutex<Option<String>> = Mutex::new(None);
static REGISTERING: Mutex<()> = Mutex::new(());

/// The message of a `jarvis_limit` refusal.
pub fn limit_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    (v.pointer("/error/code")?.as_str()? == "jarvis_limit")
        .then(|| v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("Дневной лимит Jarvis Cloud исчерпан.").to_string())
}

fn register() -> AppResult<String> {
    let http = reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).user_agent(UA).build()?;
    let res = http
        .post(format!("{BASE}/v1/register"))
        .send()
        .map_err(|e| AppError::Network(format!("Jarvis Cloud недоступен: {e}")))?;
    let status = res.status();
    let body = res.text().unwrap_or_default();
    if !status.is_success() {
        return Err(AppError::Provider(
            limit_message(&body).unwrap_or_else(|| format!("Jarvis Cloud временно не выдал токен устройства (HTTP {})", status.as_u16())),
        ));
    }
    let v: serde_json::Value = serde_json::from_str(&body)?;
    v["token"]
        .as_str()
        .filter(|t| t.starts_with("j1."))
        .map(str::to_string)
        .ok_or_else(|| AppError::Provider("Jarvis Cloud временно ответил без токена".into()))
}

/// The device token, registering this install on first use. Blocks; the
/// network call runs on its own thread, so this is safe from any context.
pub fn token_blocking() -> AppResult<String> {
    if let Some(t) = TOKEN.lock().clone() {
        return Ok(t);
    }
    let _once = REGISTERING.lock();
    if let Some(t) = TOKEN.lock().clone() {
        return Ok(t);
    }
    if let Some(t) = secrets::get(secrets::CLOUD_TOKEN).ok().flatten().filter(|t| t.starts_with("j1.")) {
        *TOKEN.lock() = Some(t.clone());
        return Ok(t);
    }
    let t = std::thread::spawn(register).join().map_err(|_| AppError::internal("Jarvis Cloud: регистрация упала"))??;
    if let Err(e) = secrets::set(secrets::CLOUD_TOKEN, &t) {
        tracing::warn!("cloud token kept in memory only: {e}");
    }
    tracing::info!("registered with Jarvis Cloud");
    *TOKEN.lock() = Some(t.clone());
    Ok(t)
}

pub async fn token() -> AppResult<String> {
    if let Some(t) = TOKEN.lock().clone() {
        return Ok(t);
    }
    tokio::task::spawn_blocking(token_blocking).await.map_err(|e| AppError::internal(e.to_string()))?
}

/// The server refused the token: get a new one on the next request.
pub fn forget_token() {
    *TOKEN.lock() = None;
    let _ = secrets::delete(secrets::CLOUD_TOKEN);
}

/// Until when cloud recognition is not tried (no network, a cap, a refusal).
static STT_OFF_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

fn stt_off_for(d: Duration) {
    *STT_OFF_UNTIL.lock() = Some(Instant::now() + d);
}

pub fn stt_available() -> bool {
    STT_OFF_UNTIL.lock().is_none_or(|t| Instant::now() >= t)
}

/// 16 kHz mono float samples as LINEAR16 little-endian bytes.
pub fn pcm16(pcm: &[f32]) -> Vec<u8> {
    pcm.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect()
}

/// Shortest and longest audio the Worker takes (0.1 s, 30 s at 16 kHz).
const STT_MIN: usize = 1_600;
const STT_MAX: usize = 30 * 16_000;

/// Recognise 16 kHz mono speech with SpeechKit through the cloud.
pub fn recognize(pcm: &[f32]) -> AppResult<String> {
    static HTTP: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    let http = match HTTP.get() {
        Some(h) => h,
        None => {
            let h = reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(10))
                .pool_idle_timeout(Duration::from_secs(90))
                .user_agent(UA)
                .build()?;
            HTTP.get_or_init(|| h)
        }
    };
    let token = token_blocking()?;
    let res = http
        .post(format!("{BASE}/v1/stt"))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(pcm16(pcm))
        .send()
        .map_err(|e| {
            stt_off_for(Duration::from_secs(60));
            AppError::Network(format!("Распознавание Jarvis Cloud недоступно: {e}"))
        })?;
    let status = res.status().as_u16();
    let body = res.text().unwrap_or_default();
    match status {
        200 => {}
        401 => {
            forget_token();
            return Err(AppError::Provider("Jarvis Cloud не принял токен устройства".into()));
        }
        429 => {
            stt_off_for(Duration::from_secs(if limit_message(&body).is_some() { 1800 } else { 60 }));
            return Err(AppError::RateLimited(60));
        }
        s => {
            stt_off_for(Duration::from_secs(600));
            return Err(AppError::Provider(format!("Распознавание Jarvis Cloud: HTTP {s}")));
        }
    }
    let v: serde_json::Value = serde_json::from_str(&body)?;
    Ok(v["text"].as_str().unwrap_or_default().trim().to_string())
}

/// How long after the request ended the cloud answer is still awaited.
const STT_DEADLINE: Duration = Duration::from_millis(2500);
/// Late answers in a row; two of them pause the cloud for half an hour, so a
/// slow line does not hold every request for the whole deadline.
static STT_LATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Recognise a finished request: SpeechKit through the cloud and the local
/// model at once. The cloud text wins when it arrives within
/// [`STT_DEADLINE`] and has words; otherwise the local one is used, so a
/// slow or absent network costs little.
pub fn transcribe_request(pcm: &[f32], local: impl FnOnce() -> AppResult<Transcript>) -> AppResult<Transcript> {
    if !stt_available() || !(STT_MIN..=STT_MAX).contains(&pcm.len()) {
        return local();
    }
    let started = Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    let audio = pcm.to_vec();
    std::thread::spawn(move || {
        let _ = tx.send(recognize(&audio));
    });
    let mine = local();
    match rx.recv_timeout(STT_DEADLINE.saturating_sub(started.elapsed())) {
        Ok(Ok(text)) if stt::is_meaningful(&text) => {
            STT_LATE.store(0, std::sync::atomic::Ordering::Relaxed);
            let ms = started.elapsed().as_millis() as u64;
            tracing::info!(ms, local_ms = mine.as_ref().map(|t| t.ms).ok(), "speech recognised by SpeechKit");
            Ok(Transcript { text, ms })
        }
        Ok(Err(e)) => {
            tracing::debug!("cloud recognition skipped: {e}");
            mine
        }
        Ok(Ok(_)) => mine,
        Err(_) => {
            if STT_LATE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1 >= 2 {
                STT_LATE.store(0, std::sync::atomic::Ordering::Relaxed);
                stt_off_for(Duration::from_secs(1800));
                tracing::info!("cloud recognition too slow twice, local only for 30 min");
            } else {
                tracing::debug!("cloud recognition too slow, local text used");
            }
            mine
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_urls_are_recognised_by_origin() {
        assert!(is_cloud(API));
        assert!(is_cloud("https://JARVIS-cloud.credo-x5.workers.dev/v1/"));
        assert!(!is_cloud("https://api.deepseek.com/v1"));
        assert!(!is_cloud("https://jarvis-cloud.credo-x5.workers.dev.evil.com/v1"));
    }

    #[test]
    fn only_limit_refusals_carry_their_message() {
        let body = r#"{"error":{"message":"Дневной лимит","code":"jarvis_limit"}}"#;
        assert_eq!(limit_message(body).as_deref(), Some("Дневной лимит"));
        assert_eq!(limit_message(r#"{"error":{"message":"x","code":"jarvis_error"}}"#), None);
        assert_eq!(limit_message("<html>"), None);
    }

    #[test]
    fn samples_become_little_endian_pcm16_and_clip() {
        assert_eq!(pcm16(&[0.0, 1.0, -2.0]), [0, 0, 0xff, 0x7f, 0x01, 0x80]);
    }

    #[test]
    fn short_or_long_audio_stays_local() {
        let local = || Ok(Transcript { text: "локально".into(), ms: 1 });
        assert_eq!(transcribe_request(&[0.0; 100], local).unwrap().text, "локально");
        assert_eq!(transcribe_request(&vec![0.0; STT_MAX + 1], local).unwrap().text, "локально");
    }

    /// Live: registers this PC (token in the credential store), then one
    /// short chat turn and a web search through the deployed Worker.
    #[test]
    #[ignore]
    fn live_cloud_chat() {
        use crate::ai::types::{ChatMessage, Role};
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = crate::ai::OpenAiCompatible::new(API.into(), None, "deepseek-flash".into(), false, 0.3);
        assert!(client.supports_web());
        let cancel = tokio_util::sync::CancellationToken::new();
        let msgs = [ChatMessage::text(Role::User, "Ответь одним словом: столица Казахстана?")];
        let done = rt.block_on(client.complete(&msgs, &[], &cancel, Some(20), |_| {})).expect("chat");
        println!("reply={:?} usage={:?}", done.message.joined_text(), done.usage);
        let found = rt.block_on(client.deepseek_tool("search", serde_json::json!({"query":"Astana weather"}), &cancel));
        println!("search ok={}", found.is_ok());
    }

    /// Live: one second of tone through the deployed Worker (needs the network).
    #[test]
    #[ignore]
    fn live_cloud_recognition() {
        let pcm: Vec<f32> = (0..32_000).map(|i| (i as f32 * 0.05).sin() * 0.1).collect();
        println!("{:?}", recognize(&pcm));
    }
}
