//! Speech synthesis for JARVIS.
//!
//! Priority order out of the box:
//! 1. Free built-in Edge Neural HD streaming voices (`edge:ru-RU-DmitryNeural`, etc.)
//!    with warm WebSocket connection pooling (~160–260 ms first audio chunk) and
//!    automatic zero-glitch offline fallback when disconnected;
//! 2. Cloud studio voices when user API keys are configured (`eleven:*`, `azure:*`);
//! 3. Local offline neural voices via `sherpa-onnx` (`supertonic-3:*`, `piper-*`);
//! 4. Built-in Windows OneCore/SAPI voices (`windows:*`, `sapi:*`).
//!
//! Includes a 2-tier RAM (64 entries, up to 140 chars) + persistent disk LRU cache
//! in `%LOCALAPPDATA%\dev.jarvis.desktop\tts_cache` keyed by SHA-256 of
//! `(voice_id, rate_bits, text)` so repeated phrases, greetings, and UI status
//! readouts play in <10 ms across restarts.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use windows::core::HSTRING;
use windows::Media::SpeechSynthesis::{SpeechAppendedSilence, SpeechPunctuationSilence, SpeechSynthesizer};
use windows::Storage::Streams::DataReader;

use super::audio::Resampler;
use super::models::{self, Kind};
use super::neural::NeuralVoice;
use crate::error::{AppError, AppResult};

/// Maximum character length cached in the 2-tier audio cache.
pub const MAX_CACHE_CHARS: usize = 140;
/// Maximum entries retained in the hot RAM LRU tier.
pub const MAX_RAM_ENTRIES: usize = 64;
/// Maximum `.pcm` files retained in the persistent disk LRU tier.
pub const MAX_DISK_ENTRIES: usize = 256;

const DISK_MAGIC: &[u8; 8] = b"JPCM\x01\x00\x00\x00";

/// Persistent disk cache folder: `%LOCALAPPDATA%\dev.jarvis.desktop\tts_cache`.
pub fn tts_cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dev.jarvis.desktop")
        .join("tts_cache")
}

/// Deterministic SHA-256 cache key over `(voice_id, rate_bits, trimmed_text)`.
pub fn cache_key(voice_id: &str, rate_bits: u32, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(voice_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(rate_bits.to_le_bytes());
    hasher.update(b"\0");
    hasher.update(text.trim().as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 2-tier (RAM LRU + persistent disk LRU) PCM16 24 kHz audio cache.
pub struct PcmCache {
    ram: Mutex<VecDeque<(String, Vec<i16>)>>,
    disk_dir: Option<PathBuf>,
    writes_since_prune: AtomicUsize,
}

impl Default for PcmCache {
    fn default() -> Self {
        Self::with_dir(Some(tts_cache_dir()))
    }
}

impl PcmCache {
    pub fn with_dir(disk_dir: Option<PathBuf>) -> Self {
        PcmCache {
            ram: Mutex::new(VecDeque::with_capacity(MAX_RAM_ENTRIES)),
            disk_dir,
            writes_since_prune: AtomicUsize::new(0),
        }
    }

    pub fn memory_only() -> Self {
        Self::with_dir(None)
    }

    #[inline]
    fn eligible(text: &str) -> bool {
        let t = text.trim();
        !t.is_empty() && t.chars().count() <= MAX_CACHE_CHARS
    }

    /// Look up cached 24 kHz mono PCM16 in RAM first, then persistent disk.
    pub fn get(&self, voice_id: &str, rate_bits: u32, text: &str) -> Option<Vec<i16>> {
        if !Self::eligible(text) {
            return None;
        }
        let key = cache_key(voice_id, rate_bits, text);

        // Tier 1: Hot RAM LRU.
        {
            let mut ram = self.ram.lock();
            if let Some(idx) = ram.iter().position(|(k, _)| k == &key) {
                let entry = ram.remove(idx).unwrap();
                let out = entry.1.clone();
                ram.push_back(entry);
                return Some(out);
            }
        }

        // Tier 2: Persistent Disk LRU.
        let dir = self.disk_dir.as_ref()?;
        let file_path = dir.join(format!("{key}.pcm"));
        let pcm = read_disk_pcm(&file_path)?;

        // Promote into RAM LRU.
        let mut ram = self.ram.lock();
        if !ram.iter().any(|(k, _)| k == &key) {
            if ram.len() >= MAX_RAM_ENTRIES {
                ram.pop_front();
            }
            ram.push_back((key, pcm.clone()));
        }
        Some(pcm)
    }

    /// Store 24 kHz mono PCM16 into both RAM LRU and persistent disk cache.
    pub fn put(&self, voice_id: &str, rate_bits: u32, text: &str, pcm: &[i16]) {
        if pcm.is_empty() || !Self::eligible(text) {
            return;
        }
        let key = cache_key(voice_id, rate_bits, text);

        {
            let mut ram = self.ram.lock();
            if let Some(idx) = ram.iter().position(|(k, _)| k == &key) {
                ram.remove(idx);
            } else if ram.len() >= MAX_RAM_ENTRIES {
                ram.pop_front();
            }
            ram.push_back((key.clone(), pcm.to_vec()));
        }

        if let Some(dir) = &self.disk_dir {
            if write_disk_pcm(dir, &key, pcm).is_ok() {
                let n = self.writes_since_prune.fetch_add(1, Ordering::Relaxed) + 1;
                if n == 1 || n % 16 == 0 {
                    prune_disk_lru(dir, MAX_DISK_ENTRIES);
                }
            }
        }
    }

    pub fn render_keyed(
        &self,
        voice_id: &str,
        rate_bits: u32,
        text: &str,
        synth: impl FnOnce() -> AppResult<Vec<i16>>,
    ) -> AppResult<Vec<i16>> {
        if !Self::eligible(text) {
            return synth();
        }
        if let Some(pcm) = self.get(voice_id, rate_bits, text) {
            return Ok(pcm);
        }
        let pcm = synth()?;
        self.put(voice_id, rate_bits, text, &pcm);
        Ok(pcm)
    }

    /// Backward-compatible helper using default voice/rate key.
    pub fn render(&self, text: &str, synth: impl FnOnce() -> AppResult<Vec<i16>>) -> AppResult<Vec<i16>> {
        self.render_keyed("default", 1.0f32.to_bits(), text, synth)
    }
}

fn read_disk_pcm(path: &Path) -> Option<Vec<i16>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 12 || &bytes[0..8] != DISK_MAGIC {
        return None;
    }
    let count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let payload = bytes.get(12..)?;
    if count == 0 || payload.len() != count * 2 {
        return None;
    }
    Some(
        payload
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect(),
    )
}

fn write_disk_pcm(dir: &Path, key: &str, pcm: &[i16]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut buf = Vec::with_capacity(12 + pcm.len() * 2);
    buf.extend_from_slice(DISK_MAGIC);
    buf.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    for &s in pcm {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    let tmp = dir.join(format!("{key}.{}.tmp", std::process::id()));
    let dst = dir.join(format!("{key}.pcm"));
    std::fs::write(&tmp, &buf)?;
    if std::fs::rename(&tmp, &dst).is_err() {
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(&dst, &buf)?;
    }
    Ok(())
}

fn prune_disk_lru(dir: &Path, max_entries: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("pcm"))
        .filter_map(|p| {
            let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((mtime, p))
        })
        .collect();
    if files.len() <= max_entries {
        return;
    }
    files.sort_by_key(|(mtime, _)| *mtime);
    let excess = files.len() - max_entries;
    for (_, old_path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(old_path);
    }
}

pub struct CachedSpeaker {
    pub voice_id: String,
    pub rate_bits: u32,
    pub speaker: Speaker,
    pub cache: Arc<PcmCache>,
}

impl CachedSpeaker {
    pub fn synthesize(&self, text: &str) -> AppResult<Vec<i16>> {
        self.cache.render_keyed(&self.voice_id, self.rate_bits, text, || {
            self.speaker.synthesize(text, super::audio::SPEAKER_RATE)
        })
    }

    /// Like `synthesize`, but streaming voices (`Edge`, `Eleven`) hand over
    /// audio chunks immediately as frames arrive; `sink` returns `false` to stop.
    pub fn synthesize_stream(&self, text: &str, sink: &mut dyn FnMut(&[i16]) -> bool) -> AppResult<()> {
        if let Some(pcm) = self.cache.get(&self.voice_id, self.rate_bits, text) {
            sink(&pcm);
            return Ok(());
        }
        if !matches!(self.speaker, Speaker::Edge(..) | Speaker::Eleven(..)) {
            let pcm = self.synthesize(text)?;
            sink(&pcm);
            return Ok(());
        }
        let mut all = Vec::new();
        let mut complete = true;
        self.speaker.synthesize_stream(text, &mut |pcm| {
            all.extend_from_slice(pcm);
            complete = sink(pcm);
            complete
        })?;
        if complete && !all.is_empty() {
            self.cache.put(&self.voice_id, self.rate_bits, text, &all);
        }
        Ok(())
    }
}

pub struct SpeechEngine {
    cache: Arc<PcmCache>,
    slot: Mutex<Option<(String, u32, Arc<CachedSpeaker>)>>,
}

impl Default for SpeechEngine {
    fn default() -> Self {
        SpeechEngine {
            cache: Arc::new(PcmCache::default()),
            slot: Mutex::new(None),
        }
    }
}

impl SpeechEngine {
    pub fn get(&self, voice: &str, rate: f32) -> AppResult<Arc<CachedSpeaker>> {
        let effective = if voice.trim().is_empty() {
            default_voice()
        } else {
            voice.trim().to_string()
        };
        let rate_clamped = rate.clamp(0.5, 2.0);
        let rate_bits = rate_clamped.to_bits();

        let mut slot = self.slot.lock();
        if let Some((id, r, speaker)) = slot.as_ref() {
            if id == &effective && *r == rate_bits {
                return Ok(speaker.clone());
            }
        }
        let (actual_id, speaker) = match Speaker::new(&effective, rate_clamped as f64) {
            Ok(sp) => (effective.clone(), sp),
            Err(e) => {
                tracing::warn!("selected voice '{effective}' unavailable, falling back to local voice: {e}");
                let fallback_id = local_voice();
                let sp = Speaker::new(&fallback_id, rate_clamped as f64)?;
                (fallback_id, sp)
            }
        };
        let cached = Arc::new(CachedSpeaker {
            voice_id: actual_id,
            rate_bits,
            speaker,
            cache: self.cache.clone(),
        });
        *slot = Some((effective, rate_bits, cached.clone()));
        Ok(cached)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoice {
    pub id: String,
    pub name: String,
    pub language: String,
}

fn platform(e: windows::core::Error) -> AppError {
    AppError::platform(format!("Синтез речи: {}", e.message()))
}

/// Prefix of Windows voice ids in settings (`windows:<WinRT voice id>`).
pub const WINDOWS_PREFIX: &str = "windows:";

/// Supertonic 3 speakers (sid → name), default first.
pub const SUPERTONIC_SPEAKERS: &[(i32, &str)] = &[
    (6, "Мужской 7, низкий"),
    (9, "Мужской 10, низкий"),
    (5, "Мужской 6"),
    (7, "Мужской 8"),
    (8, "Мужской 9"),
    (0, "Женский 1"),
    (1, "Женский 2"),
    (2, "Женский 3"),
    (3, "Женский 4"),
    (4, "Женский 5"),
];

/// Voices for Settings: free built-in Edge Neural HD voices first, then
/// configured cloud voices, installed local neural models, SAPI, and Windows.
pub fn voices() -> AppResult<Vec<TtsVoice>> {
    let mut out = Vec::new();
    for &(id, label, lang) in super::edge::VOICES {
        out.push(TtsVoice {
            id: format!("{}{id}", super::edge::PREFIX),
            name: label.to_string(),
            language: lang.to_string(),
        });
    }
    if super::elevenlabs::configured() {
        for (id, name) in super::elevenlabs::voices() {
            out.push(TtsVoice {
                id: format!("{}{id}", super::elevenlabs::PREFIX),
                name: format!("{name} (ElevenLabs)"),
                language: "ru-RU".into(),
            });
        }
    }
    if super::azure::configured() {
        for (id, name) in super::azure::VOICES {
            out.push(TtsVoice {
                id: format!("{}{id}", super::azure::PREFIX),
                name: format!("{name} (Microsoft)"),
                language: "ru-RU".into(),
            });
        }
    }
    for a in models::ASSETS.iter().filter(|a| a.kind == Kind::Tts && a.installed()) {
        if a.id.starts_with("supertonic") {
            for (sid, name) in SUPERTONIC_SPEAKERS {
                out.push(TtsVoice {
                    id: format!("{}:{sid}", a.id),
                    name: format!("{name} (Supertonic)"),
                    language: "ru-RU".into(),
                });
            }
        } else {
            out.push(TtsVoice {
                id: a.id.into(),
                name: format!("{} (Piper)", a.label.trim_start_matches("Piper ")),
                language: "ru-RU".into(),
            });
        }
    }
    for v in super::sapi::voices().into_iter().filter(|v| v.russian() && !v.name.starts_with("Microsoft")) {
        out.push(TtsVoice {
            id: format!("{}{}", super::sapi::PREFIX, v.id),
            name: format!("{} (SAPI)", v.name),
            language: "ru-RU".into(),
        });
    }
    if let Ok(win) = windows_voices() {
        out.extend(win.into_iter().map(|v| TtsVoice {
            id: format!("{WINDOWS_PREFIX}{}", v.id),
            name: format!("{} (Windows)", v.name),
            ..v
        }));
    }
    Ok(out)
}

/// Default voice when user hasn't picked one: ElevenLabs or Azure if explicitly
/// configured with keys, otherwise free built-in `edge:ru-RU-DmitryNeural`!
pub fn default_voice() -> String {
    if super::elevenlabs::configured() {
        return format!("{}{}", super::elevenlabs::PREFIX, super::elevenlabs::DEFAULT_VOICE);
    }
    if super::azure::configured() {
        return format!("{}{}", super::azure::PREFIX, super::azure::VOICES[0].0);
    }
    format!("{}{}", super::edge::PREFIX, super::edge::DEFAULT_VOICE)
}

pub enum Speaker {
    /// Free built-in Edge ReadAloud Neural HD streaming voice, with automatic
    /// local offline fallback (`NeuralVoice` → `WindowsSpeaker`) when offline.
    Edge(super::edge::EdgeVoice, std::sync::OnceLock<Option<Box<Speaker>>>, f64),
    Neural(NeuralVoice),
    Windows(WindowsSpeaker),
    /// Microsoft Azure neural voice; local fallback when network fails.
    Azure(super::azure::AzureVoice, std::sync::OnceLock<Option<Box<Speaker>>>, f64),
    /// A SAPI 5 engine such as Silero.
    Sapi(super::sapi::SapiVoice),
    /// ElevenLabs, streamed; local fallback when network fails.
    Eleven(super::elevenlabs::ElevenVoice, std::sync::OnceLock<Option<Box<Speaker>>>, f64),
}

/// Best purely offline local voice (Supertonic → Piper → Windows OneCore).
pub fn local_voice() -> String {
    if models::asset("supertonic-3").is_some_and(|a| a.installed()) {
        if let Some((sid, _)) = SUPERTONIC_SPEAKERS.first() {
            return format!("supertonic-3:{sid}");
        }
    }
    models::ASSETS
        .iter()
        .find(|a| a.kind == Kind::Tts && a.id.starts_with("piper") && a.installed())
        .map(|a| a.id.to_string())
        .unwrap_or_else(|| WINDOWS_PREFIX.to_string())
}

impl Speaker {
    /// `voice`: an id from `voices()`; empty = `default_voice()`.
    pub fn new(voice: &str, rate: f64) -> AppResult<Speaker> {
        let voice = if voice.trim().is_empty() {
            default_voice()
        } else {
            voice.trim().to_string()
        };
        if let Some(id) = voice.strip_prefix(super::edge::PREFIX) {
            return Ok(Speaker::Edge(
                super::edge::EdgeVoice::new(id, rate)?,
                std::sync::OnceLock::new(),
                rate,
            ));
        }
        if let Some(id) = voice.strip_prefix(super::elevenlabs::PREFIX) {
            return Ok(Speaker::Eleven(
                super::elevenlabs::ElevenVoice::new(id, rate)?,
                std::sync::OnceLock::new(),
                rate,
            ));
        }
        if let Some(id) = voice.strip_prefix(super::azure::PREFIX) {
            return Ok(Speaker::Azure(
                super::azure::AzureVoice::new(id, rate)?,
                std::sync::OnceLock::new(),
                rate,
            ));
        }
        if let Some(id) = voice.strip_prefix(super::sapi::PREFIX) {
            return Ok(Speaker::Sapi(super::sapi::SapiVoice::new(id, rate)?));
        }
        if let Some(id) = voice.strip_prefix(WINDOWS_PREFIX) {
            return Ok(Speaker::Windows(WindowsSpeaker::new(id, rate)?));
        }
        if voice.starts_with("HKEY_") {
            return Ok(Speaker::Windows(WindowsSpeaker::new(&voice, rate)?));
        }
        Ok(Speaker::Neural(NeuralVoice::load(&voice, rate as f32)?))
    }

    pub fn name(&self) -> String {
        match self {
            Speaker::Edge(e, ..) => e.label.clone(),
            Speaker::Neural(v) => v.name.clone(),
            Speaker::Windows(w) => format!("{} (Windows)", w.voice),
            Speaker::Azure(a, ..) => format!("{} (Microsoft)", a.voice),
            Speaker::Sapi(s) => format!("{} (SAPI)", s.name),
            Speaker::Eleven(e, ..) => format!("{} (ElevenLabs)", e.name),
        }
    }

    /// Synthesise `text` to PCM16 mono at `out_rate` (`SPEAKER_RATE` for playback).
    pub fn synthesize(&self, text: &str, out_rate: u32) -> AppResult<Vec<i16>> {
        if matches!(self, Speaker::Edge(..) | Speaker::Eleven(..)) {
            let mut all = Vec::new();
            self.synthesize_stream(text, &mut |pcm| {
                all.extend_from_slice(pcm);
                true
            })?;
            if out_rate == 24_000 {
                return Ok(all);
            }
            let mono: Vec<f32> = all.iter().map(|s| *s as f32 / 32768.0).collect();
            let mut out = Vec::new();
            Resampler::new(24_000, out_rate).process(&mono, &mut out);
            return Ok(out.iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect());
        }
        let text = super::ru_speech::normalize(text);
        let text = text.as_str();
        let (rate, mono) = match self {
            Speaker::Neural(v) => v.synthesize(text)?,
            Speaker::Windows(w) => w.synthesize_raw(text)?,
            Speaker::Sapi(s) => (
                super::sapi::RATE,
                s.synthesize(text)?.iter().map(|v| *v as f32 / 32768.0).collect(),
            ),
            Speaker::Azure(a, fallback, speed) => match a.synthesize(text) {
                Ok(pcm) if out_rate == 24_000 => return Ok(pcm),
                Ok(pcm) => (24_000, pcm.iter().map(|s| *s as f32 / 32768.0).collect()),
                Err(e) => {
                    tracing::warn!("{e}; local voice speaks instead");
                    let local = fallback.get_or_init(|| Speaker::new(&local_voice(), *speed).ok().map(Box::new));
                    return match local {
                        Some(s) => s.synthesize(text, out_rate),
                        None => Err(e),
                    };
                }
            },
            Speaker::Edge(..) | Speaker::Eleven(..) => unreachable!("handled above"),
        };
        let mono = trim_leading_silence(&mono, rate);
        let mut out = Vec::with_capacity(mono.len() * out_rate as usize / rate as usize + 8);
        Resampler::new(rate, out_rate).process(mono, &mut out);
        Ok(out.iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect())
    }

    /// Stream PCM16 mono at 24 kHz as frames arrive (`Edge` and `Eleven` stream
    /// directly; if network fails before any sound is emitted, falls back
    /// seamlessly to local offline `NeuralVoice` → `WindowsSpeaker`).
    pub fn synthesize_stream(&self, text: &str, sink: &mut dyn FnMut(&[i16]) -> bool) -> AppResult<()> {
        match self {
            Speaker::Edge(edge, fallback, speed) => {
                let normalized = super::ru_speech::normalize_for_voice(text, &edge.voice);
                let mut started = false;
                let res = edge.stream(&normalized, &mut |pcm| {
                    started = true;
                    sink(pcm)
                });
                match res {
                    Err(err) if !started => {
                        tracing::warn!("Edge neural TTS unavailable ({err}); seamlessly using local offline voice");
                        match fallback.get_or_init(|| Speaker::new(&local_voice(), *speed).ok().map(Box::new)) {
                            Some(local) => {
                                sink(&local.synthesize(text, 24_000)?);
                                Ok(())
                            }
                            None => Err(err),
                        }
                    }
                    other => other,
                }
            }
            Speaker::Eleven(e, fallback, speed) => {
                let normalized = super::ru_speech::normalize(text);
                let mut started = false;
                let res = e.stream(&normalized, &mut |pcm| {
                    started = true;
                    sink(pcm)
                });
                match res {
                    Err(err) if !started => {
                        tracing::warn!("{err}; local voice speaks instead");
                        match fallback.get_or_init(|| Speaker::new(&local_voice(), *speed).ok().map(Box::new)) {
                            Some(s) => {
                                sink(&s.synthesize(text, 24_000)?);
                                Ok(())
                            }
                            None => Err(err),
                        }
                    }
                    other => other,
                }
            }
            _ => {
                sink(&self.synthesize(text, 24_000)?);
                Ok(())
            }
        }
    }
}

/// Voices installed in Windows, Russian first.
pub fn windows_voices() -> AppResult<Vec<TtsVoice>> {
    crate::platform::com::ensure_mta();
    let mut out: Vec<TtsVoice> = SpeechSynthesizer::AllVoices()
        .map_err(platform)?
        .into_iter()
        .filter_map(|v| {
            Some(TtsVoice {
                id: v.Id().ok()?.to_string(),
                name: v.DisplayName().ok()?.to_string(),
                language: v.Language().ok()?.to_string(),
            })
        })
        .collect();
    out.sort_by_key(|v| (!v.language.eq_ignore_ascii_case("ru-RU"), v.name.clone()));
    Ok(out)
}

pub struct WindowsSpeaker {
    synth: SpeechSynthesizer,
    pub voice: String,
}

impl WindowsSpeaker {
    /// `voice_id` empty → the first Russian voice, else the system default.
    pub fn new(voice_id: &str, rate: f64) -> AppResult<WindowsSpeaker> {
        crate::platform::com::ensure_mta();
        let synth = SpeechSynthesizer::new().map_err(platform)?;
        let all: Vec<_> = SpeechSynthesizer::AllVoices().map_err(platform)?.into_iter().collect();
        let chosen = all
            .iter()
            .find(|v| !voice_id.is_empty() && v.Id().map(|id| id.to_string() == voice_id).unwrap_or(false))
            .or_else(|| all.iter().find(|v| v.Language().map(|l| l.to_string().eq_ignore_ascii_case("ru-RU")).unwrap_or(false)));
        let voice = match chosen {
            Some(v) => {
                synth.SetVoice(v).map_err(platform)?;
                v.DisplayName().map(|n| n.to_string()).unwrap_or_default()
            }
            None => SpeechSynthesizer::DefaultVoice()
                .and_then(|v| v.DisplayName())
                .map(|n| n.to_string())
                .unwrap_or_default(),
        };
        let opts = synth.Options().map_err(platform)?;
        opts.SetSpeakingRate(rate.clamp(0.5, 2.0)).map_err(platform)?;
        let _ = opts.SetAppendedSilence(SpeechAppendedSilence::Min);
        let _ = opts.SetPunctuationSilence(SpeechPunctuationSilence::Min);
        Ok(WindowsSpeaker { synth, voice })
    }

    /// Mono samples and their rate.
    pub fn synthesize_raw(&self, text: &str) -> AppResult<(u32, Vec<f32>)> {
        let stream = self
            .synth
            .SynthesizeTextToStreamAsync(&HSTRING::from(text))
            .and_then(|op| op.join())
            .map_err(platform)?;
        let size = stream.Size().map_err(platform)? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(platform)?).map_err(platform)?;
        reader.LoadAsync(size).and_then(|op| op.join()).map_err(platform)?;
        let mut wav = vec![0u8; size as usize];
        reader.ReadBytes(&mut wav).map_err(platform)?;
        parse_wav(&wav)
    }
}

/// A PCM16 mono RIFF/WAVE file.
pub fn wav_bytes(pcm: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
    w
}

/// Parse a PCM16 RIFF/WAVE file into (sample rate, mono samples).
pub fn parse_wav(b: &[u8]) -> AppResult<(u32, Vec<f32>)> {
    let bad = || AppError::platform("Синтез речи вернул неожиданный формат звука");
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(bad());
    }
    let (mut pos, mut fmt) = (12usize, None::<(u16, u16, u32, u16)>);
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32::from_le_bytes([b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]]) as usize;
        let body = &b[pos + 8..(pos + 8 + len).min(b.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                fmt = Some((
                    u16::from_le_bytes([body[0], body[1]]),
                    u16::from_le_bytes([body[2], body[3]]),
                    u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
                    u16::from_le_bytes([body[14], body[15]]),
                ));
            }
            b"data" => {
                let (format, channels, rate, bits) = fmt.ok_or_else(bad)?;
                if format != 1 || bits != 16 || channels == 0 || rate == 0 {
                    return Err(bad());
                }
                let ch = channels as usize;
                let mono = body
                    .chunks_exact(2 * ch)
                    .map(|frame| {
                        frame
                            .chunks_exact(2)
                            .map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0)
                            .sum::<f32>()
                            / ch as f32
                    })
                    .collect();
                return Ok((rate, mono));
            }
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    Err(bad())
}

/// Drop leading silence (keeping 30 ms) so speech starts sooner.
fn trim_leading_silence(s: &[f32], rate: u32) -> &[f32] {
    let first = s.iter().position(|v| v.abs() > 0.004).unwrap_or(s.len());
    let keep = (rate as usize * 30) / 1000;
    &s[first.saturating_sub(keep)..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_phrases_up_to_140_chars_use_2tier_cache_and_persist_to_disk() {
        let tmp = std::env::temp_dir().join(format!("jarvis-tts-cache-{}", uuid::Uuid::new_v4()));
        let cache = PcmCache::with_dir(Some(tmp.clone()));
        let calls = std::cell::Cell::new(0);

        for _ in 0..2 {
            assert_eq!(
                cache
                    .render_keyed("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), "Слушаю вас, сэр.", || {
                        calls.set(calls.get() + 1);
                        Ok(vec![120, -240, 360])
                    })
                    .unwrap(),
                vec![120, -240, 360]
            );
        }
        assert_eq!(calls.get(), 1);

        // Cold restart simulation: brand-new PcmCache pointing to same disk folder hits disk!
        let cold_cache = PcmCache::with_dir(Some(tmp.clone()));
        assert_eq!(
            cold_cache.get("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), "Слушаю вас, сэр."),
            Some(vec![120, -240, 360])
        );
        // Different voice or rate misses cleanly.
        assert_eq!(
            cold_cache.get("edge:ru-RU-SvetlanaNeural", 1.0f32.to_bits(), "Слушаю вас, сэр."),
            None
        );

        // Phrases <= 140 chars are cached; phrases > 140 chars bypass cache.
        for _ in 0..2 {
            cache
                .render(&"а".repeat(120), || {
                    calls.set(calls.get() + 1);
                    Ok(vec![42])
                })
                .unwrap();
        }
        assert_eq!(calls.get(), 2);

        for _ in 0..2 {
            cache
                .render(&"б".repeat(141), || {
                    calls.set(calls.get() + 1);
                    Ok(vec![1])
                })
                .unwrap();
        }
        assert_eq!(calls.get(), 4);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ram_lru_evicts_least_recently_used_past_64_entries() {
        let cache = PcmCache::memory_only();
        for i in 0..64 {
            cache.put("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), &format!("phrase-{i}"), &[i as i16]);
        }
        // Touch phrase-0 so phrase-1 becomes LRU.
        assert_eq!(cache.get("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), "phrase-0"), Some(vec![0]));
        cache.put("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), "phrase-64", &[64]);

        assert_eq!(cache.get("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), "phrase-0"), Some(vec![0]));
        assert_eq!(cache.get("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), "phrase-1"), None);
        assert_eq!(cache.get("edge:ru-RU-DmitryNeural", 1.0f32.to_bits(), "phrase-64"), Some(vec![64]));
    }

    #[test]
    fn curated_edge_voices_lead_catalog() {
        let all = voices().unwrap();
        assert!(all.len() >= 4);
        assert_eq!(all[0].id, "edge:ru-RU-DmitryNeural");
        assert_eq!(all[0].name, "Дмитрий HD · Neural (Бесплатно)");
        assert_eq!(all[1].id, "edge:ru-RU-SvetlanaNeural");
        assert_eq!(all[2].id, "edge:en-US-AndrewMultilingualNeural");
        assert_eq!(all[3].id, "edge:en-GB-RyanNeural");
    }

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        b
    }

    #[test]
    fn parses_mono_and_stereo() {
        let (rate, s) = parse_wav(&wav(22_050, 1, &[0, 16384, -16384])).unwrap();
        assert_eq!(rate, 22_050);
        assert_eq!(s, vec![0.0, 0.5, -0.5]);
        let (_, st) = parse_wav(&wav(16_000, 2, &[16384, 0, -16384, -16384])).unwrap();
        assert_eq!(st, vec![0.25, -0.5]);
        assert!(parse_wav(b"not a wav file").is_err());
    }

    #[test]
    fn wav_round_trip() {
        let (rate, s) = parse_wav(&wav_bytes(&[0, 16384, -16384], 24_000)).unwrap();
        assert_eq!((rate, s), (24_000, vec![0.0, 0.5, -0.5]));
    }

    #[test]
    fn trims_leading_silence() {
        let mut s = vec![0.0f32; 16_000];
        s.extend(std::iter::repeat_n(0.2, 100));
        assert_eq!(trim_leading_silence(&s, 16_000).len(), 480 + 100);
    }
}
