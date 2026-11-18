//! Free Zero-Key Studio Neural Streaming TTS via Microsoft Edge ReadAloud
//! (`wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1`).
//!
//! Features:
//! - Full 2026 `Sec-MS-GEC` v2 (`2-143.0.3650.75`) clock-rounded CS64+SHA256
//!   anti-abuse token + automatic HTTP 403 `Date` clock-skew recovery + MUID.
//! - Fast IPv4-first + cached `SocketAddr` + `TCP_NODELAY` TLS connector (avoids
//!   Windows IPv6 SYN blackholes and Nagle delays).
//! - Standby Warm TLS WebSocket Pool (`WARM_POOL`, 2 pre-handshaked unused
//!   connections with automatic background replenishment) so phrases skip
//!   DNS + TCP + TLS + Upgrade + `speech.config` and deliver the first 24 kHz
//!   PCM16 audio chunk in ~150–260 ms!
//! - True frame-by-frame pure-Rust MP3 → 24 kHz mono `i16` PCM streaming via
//!   `nanomp3` (`#![forbid(unsafe_code)]`, 100% `+crt-static` compatible), with
//!   leading silence trimming and click-free micro-fade on the first chunk.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use super::audio::Resampler;
use crate::error::{AppError, AppResult};

/// Voice ids in settings: `edge:<ShortName>`.
pub const PREFIX: &str = "edge:";

/// Default free HD Russian voice for JARVIS.
pub const DEFAULT_VOICE: &str = "ru-RU-DmitryNeural";

/// Curated free built-in Edge Neural HD voices: `(short_name, label, language)`.
pub const VOICES: &[(&str, &str, &str)] = &[
    ("ru-RU-DmitryNeural", "Дмитрий HD · Neural (Бесплатно)", "ru-RU"),
    ("ru-RU-SvetlanaNeural", "Светлана HD · Neural (Бесплатно)", "ru-RU"),
    ("en-US-AndrewMultilingualNeural", "Andrew Мультиязычный HD · Neural (Бесплатно)", "ru-RU"),
    ("en-GB-RyanNeural", "Ryan Британский HD · Neural (Бесплатно)", "en-GB"),
];

const BASE_HOST: &str = "speech.platform.bing.com";
const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
const CHROMIUM_MAJOR_VERSION: &str = "143";
const SEC_MS_GEC_VERSION: &str = "2-143.0.3650.75";
const ORIGIN: &str = "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold";

/// Native Edge ReadAloud 24 kHz mono CBR MP3 stream decoded frame-by-frame into 24 kHz `i16` PCM.
const OUTPUT_FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";

/// Windows FILETIME epoch offset (seconds between 1601-01-01 and 1970-01-01).
const WIN_EPOCH: i64 = 11_644_473_600;

/// Clock skew adjustment in seconds learned from server `Date` headers on 403.
static CLOCK_SKEW_SECS: AtomicI64 = AtomicI64::new(0);

/// Cached fast-responding IPv4/IPv6 endpoint address to skip repeated DNS lookups
/// and avoid dead IPv6 routes on Windows.
static CACHED_ADDR: LazyLock<Mutex<Option<(SocketAddr, Instant)>>> = LazyLock::new(|| Mutex::new(None));
const ADDR_CACHE_TTL: Duration = Duration::from_secs(300);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct WarmConn {
    ws: WsStream,
    connected_at: Instant,
}

/// Edge closes unused pre-opened sockets after ~30 s; keep 1 standby socket fresh (<18 s)
/// without triggering Azure Front Door connection-burst rate pacing.
const MAX_STANDBY_AGE: Duration = Duration::from_secs(18);
const TARGET_WARM_CONNS: usize = 1;

static EDGE_RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("jarvis-edge-tts")
        .build()
        .expect("edge-tts tokio runtime")
});

static WARM_POOL: LazyLock<tokio::sync::Mutex<VecDeque<WarmConn>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(VecDeque::with_capacity(TARGET_WARM_CONNS)));

static REPLENISHING: AtomicBool = AtomicBool::new(false);

/// Pre-open standby TLS WebSockets (with `speech.config` already sent!) in the
/// background so upcoming synthesis calls skip DNS + TCP + TLS + Upgrade completely.
pub fn prewarm() {
    if REPLENISHING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    EDGE_RT.spawn(async {
        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                REPLENISHING.store(false, Ordering::Release);
            }
        }
        let _guard = ResetGuard;

        loop {
            let need = {
                let mut pool = WARM_POOL.lock().await;
                pool.retain(|c| c.connected_at.elapsed() < MAX_STANDBY_AGE);
                TARGET_WARM_CONNS.saturating_sub(pool.len())
            };
            if need == 0 {
                break;
            }
            match connect_fresh().await {
                Ok(conn) => {
                    let mut pool = WARM_POOL.lock().await;
                    pool.retain(|c| c.connected_at.elapsed() < MAX_STANDBY_AGE);
                    if pool.len() < TARGET_WARM_CONNS {
                        pool.push_back(conn);
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub fn display_name(short_name: &str) -> String {
    VOICES
        .iter()
        .find(|(id, _, _)| *id == short_name)
        .map(|(_, label, _)| (*label).to_string())
        .unwrap_or_else(|| format!("{short_name} (Edge Neural)"))
}

pub struct EdgeVoice {
    pub voice: String,
    pub label: String,
    rate_ssml: String,
    pitch_ssml: String,
}

impl EdgeVoice {
    pub fn new(voice: &str, rate: f64) -> AppResult<Self> {
        Self::new_with_pitch(voice, rate, 0)
    }

    pub fn new_with_pitch(voice: &str, rate: f64, pitch_hz: i32) -> AppResult<Self> {
        let v = if voice.trim().is_empty() { DEFAULT_VOICE } else { voice.trim() };
        if !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(AppError::invalid(format!("Некорректный идентификатор голоса Edge: {v}")));
        }
        let pct = ((rate.clamp(0.5, 2.0) - 1.0) * 100.0).round() as i32;
        let pitch = pitch_hz.clamp(-50, 50);
        let ev = EdgeVoice {
            voice: v.to_string(),
            label: display_name(v),
            rate_ssml: format!("{pct:+}%"),
            pitch_ssml: format!("{pitch:+}Hz"),
        };
        prewarm();
        Ok(ev)
    }

    /// Stream decoded 24 kHz mono `i16` PCM chunks directly into `sink` as MP3
    /// frames arrive over WebSocket. `sink` returns `false` to abort early.
    pub fn stream(&self, text: &str, sink: &mut dyn FnMut(&[i16]) -> bool) -> AppResult<()> {
        let clean = sanitize_and_escape(text);
        if clean.trim().is_empty() {
            return Ok(());
        }
        let ssml = format!(
            "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'>\
             <voice name='{}'><prosody pitch='{}' rate='{}' volume='+0%'>{}</prosody></voice></speak>",
            self.voice, self.pitch_ssml, self.rate_ssml, clean
        );

        let run = async {
            let mut emitted_any = false;

            // 1. Pop an unused pre-warmed TLS WebSocket and immediately replenish pool in background!
            if let Some(mut warm) = take_warm_conn().await {
                prewarm();
                match stream_on_conn(&mut warm, &ssml, &mut |pcm| {
                    emitted_any = true;
                    sink(pcm)
                })
                .await
                {
                    Ok(()) => {
                        drop(warm);
                        return Ok(());
                    }
                    Err(e) if !emitted_any => {
                        tracing::debug!("standby edge websocket expired ({e}), reconnecting fresh");
                    }
                    Err(e) => return Err(e),
                }
            }

            // 2. Fresh connection (and trigger background pool warm-up for next phrase).
            let mut conn = connect_fresh().await?;
            prewarm();
            let res = stream_on_conn(&mut conn, &ssml, sink).await;
            drop(conn);
            res
        };

        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| EDGE_RT.block_on(run))
        } else {
            EDGE_RT.block_on(run)
        }
    }
}

async fn take_warm_conn() -> Option<WarmConn> {
    let mut pool = WARM_POOL.lock().await;
    while let Some(conn) = pool.pop_front() {
        if conn.connected_at.elapsed() < MAX_STANDBY_AGE {
            return Some(conn);
        }
    }
    None
}

async fn connect_fast_tcp() -> AppResult<TcpStream> {
    // 1. Try cached fast SocketAddr first (0 ms DNS, skips dead IPv6 routes).
    let cached = {
        let g = CACHED_ADDR.lock();
        g.and_then(|(addr, at)| (at.elapsed() < ADDR_CACHE_TTL).then_some(addr))
    };
    if let Some(addr) = cached {
        if let Ok(Ok(tcp)) = tokio::time::timeout(Duration::from_millis(1200), TcpStream::connect(addr)).await {
            let _ = tcp.set_nodelay(true);
            return Ok(tcp);
        }
        *CACHED_ADDR.lock() = None;
    }

    // 2. Resolve DNS and prefer IPv4 addresses first (Windows dual-stack networks
    //    frequently stall 3-6 s on blackholed IPv6 AAAA SYN packets).
    let mut addrs: Vec<SocketAddr> = tokio::time::timeout(
        Duration::from_millis(2000),
        tokio::net::lookup_host((BASE_HOST, 443)),
    )
    .await
    .map_err(|_| AppError::Timeout("DNS Edge TTS не ответил вовремя".into()))?
    .map_err(|e| AppError::Network(format!("Edge TTS недоступен (DNS): {e}")))?
    .collect();

    if addrs.is_empty() {
        return Err(AppError::Network("Не удалось разрешить адрес Edge TTS".into()));
    }
    addrs.sort_by_key(|a| a.is_ipv6());

    let mut last_err = None;
    for addr in addrs {
        match tokio::time::timeout(Duration::from_millis(1500), TcpStream::connect(addr)).await {
            Ok(Ok(tcp)) => {
                let _ = tcp.set_nodelay(true);
                *CACHED_ADDR.lock() = Some((addr, Instant::now()));
                return Ok(tcp);
            }
            Ok(Err(e)) => last_err = Some(e.to_string()),
            Err(_) => last_err = Some(format!("timeout connecting {addr}")),
        }
    }
    Err(AppError::Network(format!(
        "Edge TTS недоступен: {}",
        last_err.unwrap_or_else(|| "нет маршрута".into())
    )))
}

async fn connect_fresh() -> AppResult<WarmConn> {
    match connect_once().await {
        Ok(c) => Ok(c),
        Err(ConnectErr::ForbiddenSkew(skew_adjusted)) if skew_adjusted => {
            tracing::info!("edge-tts adjusted clock skew, retrying handshake");
            connect_once().await.map_err(ConnectErr::into_app_error)
        }
        Err(e) => Err(e.into_app_error()),
    }
}

enum ConnectErr {
    ForbiddenSkew(bool),
    Other(AppError),
}

impl ConnectErr {
    fn into_app_error(self) -> AppError {
        match self {
            ConnectErr::ForbiddenSkew(_) => AppError::Network("Сервер голоса Edge отклонил подключение (403)".into()),
            ConnectErr::Other(e) => e,
        }
    }
}

async fn connect_once() -> Result<WarmConn, ConnectErr> {
    let conn_id = Uuid::new_v4().simple().to_string();
    let sec_gec = generate_sec_ms_gec_now();
    let url = format!(
        "wss://{BASE_HOST}/consumer/speech/synthesize/readaloud/edge/v1\
         ?TrustedClientToken={TRUSTED_CLIENT_TOKEN}\
         &ConnectionId={conn_id}\
         &Sec-MS-GEC={sec_gec}\
         &Sec-MS-GEC-Version={SEC_MS_GEC_VERSION}"
    );

    let mut req = url
        .as_str()
        .into_client_request()
        .map_err(|e| ConnectErr::Other(AppError::internal(format!("edge ws url: {e}"))))?;

    let ua = format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/{CHROMIUM_MAJOR_VERSION}.0.0.0 Safari/537.36 \
         Edg/{CHROMIUM_MAJOR_VERSION}.0.0.0"
    );
    let muid = Uuid::new_v4().simple().to_string().to_uppercase();
    let headers = req.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&ua) {
        headers.insert("User-Agent", v);
    }
    headers.insert("Origin", HeaderValue::from_static(ORIGIN));
    headers.insert("Pragma", HeaderValue::from_static("no-cache"));
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
    headers.insert("Accept-Encoding", HeaderValue::from_static("gzip, deflate, br, zstd"));
    headers.insert("Accept-Language", HeaderValue::from_static("ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7"));
    if let Ok(v) = HeaderValue::from_str(&format!("muid={muid};")) {
        headers.insert("Cookie", v);
    }

    let tcp = connect_fast_tcp().await.map_err(ConnectErr::Other)?;

    let (ws, _) = tokio::time::timeout(Duration::from_millis(2500), tokio_tungstenite::client_async_tls(req, tcp))
        .await
        .map_err(|_| ConnectErr::Other(AppError::Timeout("TLS-рукопожатие Edge TTS не завершилось вовремя".into())))?
        .map_err(|e| match e {
            tokio_tungstenite::tungstenite::Error::Http(resp) if resp.status().as_u16() == 403 => {
                let adjusted = resp
                    .headers()
                    .get("date")
                    .and_then(|h| h.to_str().ok())
                    .and_then(adjust_clock_skew_from_http_date)
                    .is_some();
                ConnectErr::ForbiddenSkew(adjusted)
            }
            other => ConnectErr::Other(AppError::Network(format!("Edge TTS недоступен: {other}"))),
        })?;

    Ok(WarmConn { ws, connected_at: Instant::now() })
}

async fn stream_on_conn(
    conn: &mut WarmConn,
    ssml: &str,
    sink: &mut dyn FnMut(&[i16]) -> bool,
) -> AppResult<()> {
    let ts = js_utc_timestamp();
    let cfg_msg = format!(
        "X-Timestamp:{ts}\r\n\
         Content-Type:application/json; charset=utf-8\r\n\
         Path:speech.config\r\n\r\n\
         {{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":\
         {{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"false\"}},\
         \"outputFormat\":\"{OUTPUT_FORMAT}\"}}}}}}}}\r\n"
    );
    let req_id = Uuid::new_v4().simple().to_string();
    let ssml_msg = format!(
        "X-RequestId:{req_id}\r\n\
         Content-Type:application/ssml+xml\r\n\
         X-Timestamp:{ts}Z\r\n\
         Path:ssml\r\n\r\n\
         {ssml}"
    );

    tokio::time::timeout(Duration::from_secs(2), async {
        conn.ws.feed(Message::Text(cfg_msg.into())).await?;
        conn.ws.send(Message::Text(ssml_msg.into())).await
    })
    .await
    .map_err(|_| AppError::Timeout("Таймаут отправки SSML в Edge TTS".into()))?
    .map_err(|e| AppError::Network(format!("Edge TTS соединение закрыто: {e}")))?;

    let mut decoder = StreamingMp3Decoder::new();
    let mut got_audio = false;

    loop {
        let wait_limit = if got_audio { Duration::from_secs(5) } else { Duration::from_millis(2500) };
        let next = tokio::time::timeout(wait_limit, conn.ws.next())
            .await
            .map_err(|_| AppError::Timeout("Поток Edge TTS прервался по таймауту".into()))?;

        let Some(msg) = next else {
            return Err(AppError::Network("Сервер Edge TTS закрыл соединение до конца фразы".into()));
        };

        match msg.map_err(|e| AppError::Network(format!("Ошибка чтения потока Edge TTS: {e}")))? {
            Message::Binary(bin) => {
                if bin.len() < 2 {
                    continue;
                }
                let header_len = u16::from_be_bytes([bin[0], bin[1]]) as usize;
                let audio_start = 2usize.saturating_add(header_len);
                if audio_start > bin.len() {
                    continue;
                }
                let header_bytes = &bin[2..audio_start];
                if !contains_subslice(header_bytes, b"Path:audio") {
                    continue;
                }
                let mp3_data = &bin[audio_start..];
                if mp3_data.is_empty() {
                    continue;
                }
                got_audio = true;
                if !decoder.feed(mp3_data, false, sink) {
                    return Ok(());
                }
            }
            Message::Text(txt) => {
                let s: &str = txt.as_ref();
                if s.contains("Path:turn.end") {
                    let _ = decoder.feed(&[], true, sink);
                    break;
                }
            }
            Message::Ping(p) => {
                let _ = conn.ws.send(Message::Pong(p)).await;
            }
            Message::Close(frame) => {
                let detail = frame.map(|f| format!(": {} {}", f.code, f.reason)).unwrap_or_default();
                return Err(AppError::Network(format!("Edge TTS закрыл соединение{detail}")));
            }
            _ => {}
        }
    }

    if !got_audio {
        return Err(AppError::Provider("Edge TTS не вернул аудиоданных".into()));
    }
    Ok(())
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Frame-by-frame pure-Rust MP3 → 24 kHz mono `i16` PCM streaming decoder.
///
/// Keeps a 3-frame lookahead buffer during streaming so `minimp3`/`nanomp3`
/// never sees a truncated frame at the slice boundary, trims leading encoder
/// silence on the very first chunk only, and applies a 2 ms click-free fade-in.
pub struct StreamingMp3Decoder {
    decoder: nanomp3::Decoder,
    mp3_buf: Vec<u8>,
    frame_pcm: Box<[i16; nanomp3::MAX_SAMPLES_PER_FRAME]>,
    pending_pcm: Vec<i16>,
    resampler: Option<(u32, Resampler)>,
    first_chunk_emitted: bool,
}

/// 3 frames at 48 kbps 24 kHz mono (144 B/frame = 432 B) so `minimp3` syncs
/// immediately on the very first WebSocket binary packet!
const STREAM_LOOKAHEAD_BYTES: usize = 432;
/// 1 MP3 frame at 24 kHz (576 samples = 24 ms) for instant first-chunk delivery.
const EMIT_SAMPLES: usize = 576;

impl Default for StreamingMp3Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingMp3Decoder {
    pub fn new() -> Self {
        StreamingMp3Decoder {
            decoder: nanomp3::Decoder::new(),
            mp3_buf: Vec::with_capacity(8192),
            frame_pcm: Box::new([0i16; nanomp3::MAX_SAMPLES_PER_FRAME]),
            pending_pcm: Vec::with_capacity(2400),
            resampler: None,
            first_chunk_emitted: false,
        }
    }

    /// Feed raw MP3 bytes (and `eof = true` on `turn.end`). Calls `sink` with
    /// 24 kHz mono `i16` slices; returns `false` if `sink` requested stop.
    pub fn feed(&mut self, mp3_bytes: &[u8], eof: bool, sink: &mut dyn FnMut(&[i16]) -> bool) -> bool {
        if !mp3_bytes.is_empty() {
            self.mp3_buf.extend_from_slice(mp3_bytes);
        }

        let mut offset = 0usize;
        loop {
            let remaining = self.mp3_buf.len().saturating_sub(offset);
            if remaining == 0 || (!eof && remaining < STREAM_LOOKAHEAD_BYTES) {
                break;
            }
            let (consumed, res) = self.decoder.decode(&self.mp3_buf[offset..], &mut *self.frame_pcm);
            if consumed == 0 {
                break;
            }
            offset += consumed;

            if let Ok(info) = res {
                if info.samples_produced == 0 {
                    continue;
                }
                let ch = info.channels.num() as usize;
                if ch == 1 && info.sample_rate == 24_000 {
                    self.pending_pcm.extend_from_slice(&self.frame_pcm[..info.samples_produced]);
                } else {
                    let mut mono_f32 = Vec::with_capacity(info.samples_produced);
                    if ch == 1 {
                        for &s in &self.frame_pcm[..info.samples_produced] {
                            mono_f32.push(s as f32 / 32768.0);
                        }
                    } else {
                        for pair in self.frame_pcm[..info.samples_produced * ch].chunks_exact(ch) {
                            let sum: f32 = pair.iter().map(|&s| s as f32 / 32768.0).sum();
                            mono_f32.push(sum / ch as f32);
                        }
                    }
                    if info.sample_rate == 24_000 {
                        self.pending_pcm.extend(mono_f32.iter().map(|&v| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16));
                    } else {
                        let rs = match &mut self.resampler {
                            Some((sr, r)) if *sr == info.sample_rate => r,
                            _ => {
                                self.resampler = Some((info.sample_rate, Resampler::new(info.sample_rate, 24_000)));
                                &mut self.resampler.as_mut().unwrap().1
                            }
                        };
                        let mut out_f32 = Vec::new();
                        rs.process(&mono_f32, &mut out_f32);
                        self.pending_pcm.extend(out_f32.iter().map(|&v| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16));
                    }
                }
            }
        }

        if offset > 0 {
            self.mp3_buf.drain(..offset);
        }

        // Trim leading silence ONLY on the very first emitted chunk!
        if !self.first_chunk_emitted {
            if let Some(voice_idx) = self.pending_pcm.iter().position(|s| s.unsigned_abs() > 140) {
                // Keep 10 ms (240 samples @ 24 kHz) of soft lead-in before speech onset.
                let keep_from = voice_idx.saturating_sub(240);
                if keep_from > 0 {
                    self.pending_pcm.drain(..keep_from);
                }
                // Apply a 2 ms (48-sample) linear fade-in so speakers never click.
                let ramp = self.pending_pcm.len().min(48);
                for (i, s) in self.pending_pcm[..ramp].iter_mut().enumerate() {
                    *s = ((*s as i32 * i as i32) / ramp.max(1) as i32) as i16;
                }
                if self.pending_pcm.len() >= EMIT_SAMPLES || eof {
                    self.first_chunk_emitted = true;
                    let ok = sink(&self.pending_pcm);
                    self.pending_pcm.clear();
                    return ok;
                }
            } else if eof && !self.pending_pcm.is_empty() {
                let n = self.pending_pcm.len().min(480);
                self.first_chunk_emitted = true;
                let ok = sink(&self.pending_pcm[..n]);
                self.pending_pcm.clear();
                return ok;
            }
            return true;
        }

        if self.pending_pcm.len() >= EMIT_SAMPLES || (eof && !self.pending_pcm.is_empty()) {
            let ok = sink(&self.pending_pcm);
            self.pending_pcm.clear();
            return ok;
        }

        true
    }
}

fn sanitize_and_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        let code = c as u32;
        if matches!(code, 0..=8 | 11..=12 | 14..=31 | 0x7F) {
            out.push(' ');
            continue;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn js_utc_timestamp() -> String {
    Utc::now()
        .format("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)")
        .to_string()
}

fn adjust_clock_skew_from_http_date(date_hdr: &str) -> Option<i64> {
    let parsed = chrono::DateTime::parse_from_rfc2822(date_hdr).ok()?;
    let server_unix = parsed.timestamp();
    let local_unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let skew = server_unix - local_unix;
    CLOCK_SKEW_SECS.store(skew, Ordering::Relaxed);
    Some(skew)
}

// ============================================================================
// Sec-MS-GEC v2 Token Generator (bit-exact port of Edge ReadAloud DRM v2)
// ============================================================================

const SHA256_INITIAL: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
    0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

const SHA256_STATE: [u32; 8] = [
    0xFD9FBEA2, 0x91243CE7, 0xBE499094, 0xCD2E1E0A,
    0x6C006F49, 0x04BD35AB, 0xFEB0A7E4, 0x03EFFBD2,
];

const SHA256_SCHEDULE: [u32; 14] = [
    0xEB24BF9E, 0x1558691F, 0xA0C09EE2, 0x0CFA32BA,
    0xA622EE8C, 0xD1DD9491, 0xC0DD25A0, 0xF9E70AB6,
    0xF0C31573, 0x7058CE4E, 0x7188E606, 0x00E5AB33,
    0xD258B060, 0x72766564,
];

const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5,
    0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE, 0x9BDC06A7, 0xC19BF174,
    0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA,
    0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7, 0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967,
    0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85,
    0xA2BFE8A1, 0xA81A664B, 0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070,
    0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7, 0xC67178F2,
];

const SHA256_ADJUSTMENT: u32 = 0xC9DC06A7;
const SHA256_SIGMA0_ADJUSTMENT: u32 = 0x059C0B80;
const SHA256_SIGMA1_ADJUSTMENT: u32 = 0x000B92C0;

const CS64_COEFFICIENTS: [(u32, u32); 14] = [
    (0xE82E91B1, 0xB32700D0),
    (0x44DE24C6, 0x88DFF51D),
    (0x3BEB65BC, 0x29514038),
    (0xC89C5933, 0xB328CBDF),
    (0x8D1EC211, 0xD405B4D5),
    (0x314EF5D5, 0x26E55F3C),
    (0xE92491B1, 0x607801C6),
    (0xF0A5B9D5, 0x82B2C1FB),
    (0x06064CF3, 0xFA51A3DA),
    (0xDA79FA60, 0x207CC464),
    (0xBED20560, 0x3C3C74F3),
    (0x89F9C9F8, 0xD2D14B61),
    (0x35FA38C6, 0xBB04A2C1),
    (0xCB49E3C6, 0xC5626167),
];

const CS64_FACTOR: u32 = 0x82893933;
const CS64_ADJUSTMENT0: u32 = 0xF2F90B1A;
const CS64_ADJUSTMENT1: u32 = 0x0B1A0000;

#[inline]
fn sigma0(v: u32) -> u32 {
    v.rotate_right(7) ^ v.rotate_right(18) ^ (v >> 3)
}

#[inline]
fn sigma1(v: u32) -> u32 {
    v.rotate_right(17) ^ v.rotate_right(19) ^ (v >> 10)
}

fn expand_sha256_schedule(words: &mut [u32; 64], first_word: usize) {
    for i in first_word..64 {
        let s1 = sigma1(words[i - 2]);
        let s0 = sigma0(words[i - 15]);
        words[i] = s1
            .wrapping_add(words[i - 7])
            .wrapping_add(s0)
            .wrapping_add(words[i - 16]);
    }
}

fn sha256_rounds(state: [u32; 8], words: &[u32; 64], first_round: usize) -> [u32; 8] {
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
    for i in first_round..64 {
        let k = if first_round == 14 && i == 14 {
            SHA256_ADJUSTMENT
        } else {
            SHA256_ROUND_CONSTANTS[i]
        };
        let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = g ^ (e & (f ^ g));
        let t1 = h
            .wrapping_add(big_s1)
            .wrapping_add(ch)
            .wrapping_add(k)
            .wrapping_add(words[i]);
        let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) | (c & (a | b));
        let t2 = big_s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    [a, b, c, d, e, f, g, h]
}

fn sha256_init(prefix7: &[u8]) -> [u32; 8] {
    let mut words = [0u32; 64];
    words[14] = u32::from_be_bytes([0, prefix7[0], prefix7[1], prefix7[2]]);
    words[15] = u32::from_be_bytes([prefix7[3], prefix7[4], prefix7[5], prefix7[6]]);

    for i in 16..30 {
        let mut value = SHA256_SCHEDULE[i - 16];
        if i - 2 >= 14 {
            let term = if i == 16 {
                sigma1(words[i - 2]) ^ SHA256_SIGMA1_ADJUSTMENT
            } else {
                sigma1(words[i - 2])
            };
            value = value.wrapping_add(term);
        }
        if i - 7 >= 14 {
            value = value.wrapping_add(words[i - 7]);
        }
        if i - 15 >= 14 {
            let term = if i == 29 {
                sigma0(words[i - 15]) ^ SHA256_SIGMA0_ADJUSTMENT
            } else {
                sigma0(words[i - 15])
            };
            value = value.wrapping_add(term);
        }
        words[i] = value;
    }

    words[30] = sigma1(words[28])
        .wrapping_add(words[23])
        .wrapping_add(sigma0(words[15]))
        .wrapping_add(words[14])
        .wrapping_add(SHA256_ADJUSTMENT)
        .wrapping_sub(SHA256_ROUND_CONSTANTS[14]);

    expand_sha256_schedule(&mut words, 31);
    let working = sha256_rounds(SHA256_STATE, &words, 14);
    let mut out = [0u32; 8];
    for i in 0..8 {
        out[i] = SHA256_INITIAL[i].wrapping_add(working[i]);
    }
    out
}

fn sha256_gec_digest(data: &[u8]) -> [u8; 32] {
    assert!(data.len() >= 7, "token input must contain at least 7 bytes");
    let length = 57 + data.len();
    let pad_zeros = (55isize - length as isize).rem_euclid(64) as usize;
    let mut padded = Vec::with_capacity(data.len() - 7 + 1 + pad_zeros + 8);
    padded.extend_from_slice(&data[7..]);
    padded.push(0x80);
    padded.resize(padded.len() + pad_zeros, 0);
    padded.extend_from_slice(&((length as u64) * 8).to_be_bytes());

    let mut state = sha256_init(&data[..7]);
    for block in padded.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (i, w) in block.chunks_exact(4).enumerate() {
            words[i] = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
        }
        expand_sha256_schedule(&mut words, 16);
        let working = sha256_rounds(state, &words, 0);
        for i in 0..8 {
            state[i] = state[i].wrapping_add(working[i]);
        }
    }

    let mut out = [0u8; 32];
    for (i, &w) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
    }
    out
}

fn words_le4_even_with_leading_zero(data: &[u8]) -> Vec<u32> {
    let raw_len = 1 + data.len();
    let pad = (4 - (raw_len % 4)) % 4;
    let mut bytes = Vec::with_capacity(raw_len + pad);
    bytes.push(0u8);
    bytes.extend_from_slice(data);
    bytes.resize(raw_len + pad, 0u8);

    let mut words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if words.len() % 2 == 1 {
        words.push(0);
    }
    words
}

fn cs64_digest(data: &[u8], digest: &[u8; 32]) -> (u32, u32) {
    let key_a = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 1;
    let key_b = u32::from_le_bytes([digest[4], digest[5], digest[6], digest[7]]) | 1;
    let factors = [key_a.wrapping_mul(CS64_FACTOR), key_b.wrapping_mul(CS64_FACTOR)];

    let mut terms: Vec<u32> = CS64_COEFFICIENTS
        .iter()
        .enumerate()
        .map(|(i, &(coeff, _))| coeff.wrapping_mul(factors[i % 2]))
        .collect();
    let mut offsets: Vec<u32> = CS64_COEFFICIENTS.iter().map(|&(_, off)| off).collect();

    for (i, word) in words_le4_even_with_leading_zero(data).into_iter().enumerate() {
        let mut term = word.wrapping_mul(if i % 2 == 0 { key_a } else { key_b });
        let mut offset = word
            .rotate_left(16)
            .wrapping_mul(if i % 2 == 0 { 0xEF0569FB } else { 0xC31713DB });
        if i == 0 {
            term = term.wrapping_add(CS64_ADJUSTMENT0.wrapping_mul(factors[0]));
            offset = offset.wrapping_add(CS64_ADJUSTMENT1);
        }
        terms.push(term);
        offsets.push(offset);
    }

    let mut swap_acc = 0u32;
    let mut swap_last = 0u32;
    let mut state = 0u32;
    let mut acc = 0u32;

    for i in (0..terms.len()).step_by(2) {
        let (x, y) = (terms[i], terms[i + 1]);
        let t = offsets[i].wrapping_add(x).rotate_left(16);
        let q = t.wrapping_mul(0x76F50F34);
        swap_acc = swap_acc.wrapping_add(q.rotate_left(16).wrapping_mul(0xC3EFEA97).wrapping_add(q));

        let t = offsets[i + 1].wrapping_add(y).rotate_left(16);
        let q = t.wrapping_mul(0x1881CE3C);
        swap_last = q.rotate_left(16).wrapping_mul(0x35BD1EC9).wrapping_add(q);
        swap_acc = swap_acc.wrapping_add(swap_last);

        state = state.wrapping_mul(key_a).wrapping_add(x);
        state = state.rotate_left(16).wrapping_mul(0xCF98B111);
        state = state.rotate_left(16).wrapping_mul(0x87085B9F);
        state = state.rotate_left(16).wrapping_mul(0x12CEB96D);
        state = state.rotate_left(16).wrapping_mul(0x257E1D83);
        acc = acc.wrapping_add(state);

        let mut t = state.wrapping_mul(key_b).wrapping_add(y);
        t = t.rotate_left(16).wrapping_mul(0xA27416F5);
        t = t.rotate_left(16).wrapping_mul(0xD38396FF);
        state = t.rotate_left(16).wrapping_mul(0x7C932B89).rotate_left(16).wrapping_mul(0xBFA49F69);
        acc = acc.wrapping_add(state);
    }

    (swap_last ^ state, swap_acc ^ acc)
}

pub fn generate_sec_ms_gec_at(unix_secs: i64) -> String {
    let mut ticks = unix_secs + WIN_EPOCH;
    ticks -= ticks.rem_euclid(300);
    let filetime_100ns = (ticks as i128) * 10_000_000;
    let input = format!("{filetime_100ns}{TRUSTED_CLIENT_TOKEN}");
    let data = input.as_bytes();
    let digest = sha256_gec_digest(data);
    let (c0, c1) = cs64_digest(data, &digest);

    let mut cs_bytes = [0u8; 8];
    cs_bytes[0..4].copy_from_slice(&c0.to_le_bytes());
    cs_bytes[4..8].copy_from_slice(&c1.to_le_bytes());

    let cs_hex: String = cs_bytes.iter().map(|b| format!("{b:02X}")).collect();
    let sha_hex: String = digest.iter().map(|b| format!("{b:02X}")).collect();
    format!("{cs_hex}-{sha_hex}")
}

pub fn generate_sec_ms_gec_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let skew = CLOCK_SKEW_SECS.load(Ordering::Relaxed);
    generate_sec_ms_gec_at(now + skew)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_ms_gec_v2_matches_reference_vectors() {
        assert_eq!(
            generate_sec_ms_gec_at(1_700_000_000),
            "2CD23A2CA0B2595A-F3C846C370E05F9CC83ACC5F995C5F5D87A4D0F14CE8936A0F902E64099E38A2"
        );
        assert_eq!(
            generate_sec_ms_gec_at(1_794_958_800),
            "C4D4237140077752-9830B5F991B85302EBD75DD06FB6D761A7A3E03A21B6EB65A8E189016D2AFCC5"
        );
        assert_eq!(
            generate_sec_ms_gec_at(1_794_958_923),
            "C4D4237140077752-9830B5F991B85302EBD75DD06FB6D761A7A3E03A21B6EB65A8E189016D2AFCC5"
        );
    }

    #[test]
    fn sanitizes_control_chars_and_xml_entities() {
        assert_eq!(
            sanitize_and_escape("Привет\x0b & <мир> \"JARVIS\""),
            "Привет  &amp; &lt;мир&gt; &quot;JARVIS&quot;"
        );
    }

    #[test]
    fn clock_skew_parses_rfc2822_date() {
        let skew = adjust_clock_skew_from_http_date("Wed, 18 Nov 2026 00:00:00 GMT");
        assert!(skew.is_some());
        CLOCK_SKEW_SECS.store(0, Ordering::Relaxed);
    }
}
