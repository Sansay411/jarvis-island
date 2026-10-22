//! Local speech recognition: whisper.cpp (via whisper-rs) on the CPU.
//!
//! Models are not bundled: the user downloads one from Settings. Files come
//! from the official whisper.cpp repository on Hugging Face and are checked
//! against the size and SHA-256 published there (LFS oid) before use.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState, WhisperVadContext, WhisperVadContextParams};

use crate::error::{AppError, AppResult};

pub struct SttModel {
    pub id: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub base_url: &'static str,
}

/// Sizes and hashes from huggingface.co/api/models/ggerganov/whisper.cpp/tree/main.
pub const MODELS: &[SttModel] = &[
    SttModel {
        id: "turbo",
        label: "Whisper large-v3-turbo — точнее",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        base_url: WHISPER_BASE,
    },
    SttModel {
        id: "small",
        label: "Whisper small — быстрее",
        file: "ggml-small-q8_0.bin",
        size: 264_464_607,
        sha256: "49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f",
        base_url: WHISPER_BASE,
    },
];

const WHISPER_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

/// Silero VAD for whisper.cpp (huggingface.co/ggml-org/whisper-vad): rejects
/// noise, music and clicks that whisper would otherwise "transcribe".
pub const VAD: SttModel = SttModel {
    id: "silero",
    label: "Silero VAD v6.2",
    file: "ggml-silero-v6.2.0.bin",
    size: 885_098,
    sha256: "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987",
    base_url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/",
};

pub fn model(id: &str) -> &'static SttModel {
    MODELS.iter().find(|m| m.id == id).unwrap_or(&MODELS[0])
}

pub fn models_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("dev.jarvis.desktop").join("models")
}

pub fn model_path(m: &SttModel) -> PathBuf {
    models_dir().join(m.file)
}

/// A complete download has exactly the published size (the hash was checked
/// before the file got its final name).
fn is_present(m: &SttModel) -> bool {
    std::fs::metadata(model_path(m)).map(|md| md.len() == m.size).unwrap_or(false)
}

/// The recognition model and the voice-activity model are both on disk.
pub fn is_installed(m: &SttModel) -> bool {
    is_present(m) && is_present(&VAD)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub installed: bool,
}

pub fn list() -> Vec<ModelInfo> {
    MODELS
        .iter()
        .map(|m| ModelInfo { id: m.id, label: m.label, file: m.file, size: m.size, installed: is_installed(m) })
        .collect()
}

/// Download `m` (and the VAD model if missing), reporting `(downloaded, total)`
/// for `m`.
pub async fn download(m: &'static SttModel, cancel: CancellationToken, progress: impl Fn(u64, u64)) -> AppResult<PathBuf> {
    if !is_present(&VAD) {
        download_file(&VAD, cancel.clone(), |_, _| {}).await?;
    }
    if is_present(m) {
        return Ok(model_path(m));
    }
    download_file(m, cancel, progress).await
}

/// Writes to `.part`, verifies size and SHA-256, then renames.
async fn download_file(m: &SttModel, cancel: CancellationToken, progress: impl Fn(u64, u64)) -> AppResult<PathBuf> {
    let dir = models_dir();
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Storage(format!("Папка моделей: {e}")))?;
    let final_path = model_path(m);
    let part = dir.join(format!("{}.part", m.file));
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::internal(e.to_string()))?;
    let resp = client
        .get(format!("{}{}", m.base_url, m.file))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| AppError::Network(format!("Не удалось скачать модель: {}", e.without_url())))?;
    let mut file = std::fs::File::create(&part).map_err(|e| AppError::Storage(format!("Не удалось записать модель: {e}")))?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    let mut last_report = Instant::now();
    let result: AppResult<()> = loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => break Err(AppError::Cancelled),
            n = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next()) => n,
        };
        match next {
            Err(_) => break Err(AppError::Timeout("Загрузка модели остановилась: нет данных 60 с".into())),
            Ok(None) => break Ok(()),
            Ok(Some(Err(e))) => break Err(AppError::Network(format!("Загрузка прервалась: {}", e.without_url()))),
            Ok(Some(Ok(chunk))) => {
                hasher.update(&chunk);
                if let Err(e) = file.write_all(&chunk) {
                    break Err(AppError::Storage(format!("Не удалось записать модель: {e}")));
                }
                done += chunk.len() as u64;
                if last_report.elapsed().as_millis() >= 200 {
                    last_report = Instant::now();
                    progress(done, m.size);
                }
            }
        }
    };
    drop(file);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    progress(done, m.size);
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != m.size || digest != m.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::Network(format!(
            "Файл модели повреждён (получено {done} байт, контрольная сумма не совпала) — попробуйте ещё раз"
        )));
    }
    std::fs::rename(&part, &final_path).map_err(|e| AppError::Storage(format!("Не удалось сохранить модель: {e}")))?;
    Ok(final_path)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub text: String,
    /// Highest no-speech probability among segments (0..1).
    pub no_speech: f32,
    /// Speech found by the VAD model, ms (None without it).
    pub speech_ms: Option<u64>,
    pub ms: u64,
}

/// Less speech than this is a click, cough or noise, not a command.
const MIN_SPEECH_MS: u64 = 200;

pub struct Recognizer {
    pub path: PathBuf,
    state: WhisperState,
    vad: Option<WhisperVadContext>,
    pub threads: i32,
    /// Encoder window in mel frames (50 per second, 1500 = 30 s);
    /// 0 = sized to the utterance (`adaptive_ctx`).
    pub audio_ctx: i32,
}

/// Whisper's well-known output on silence or noise in Russian (subtitle
/// credits from its training data). Such lines are never real commands.
const HALLUCINATIONS: &[&str] = &[
    "продолжение следует",
    "субтитры",
    "редактор субтитров",
    "корректор",
    "спасибо за просмотр",
    "подписывайтесь",
    "подпишись",
    "dimatorzok",
    "амара",
];

/// Whisper always encodes a 30 s window; a command lasts a few seconds.
/// Encoding only the utterance plus 2 s (at least ~10 s) is 3x faster on
/// the CPU with the same text in our tests; shorter windows start to
/// degrade (Latin "Jarvis", repetitions).
pub fn adaptive_ctx(secs: f32) -> i32 {
    let frames = ((secs + 2.0) * 50.0).ceil() as i32;
    let rounded = (frames + 127) / 128 * 128;
    rounded.clamp(512, 1500)
}

fn load_vad(threads: i32) -> Option<WhisperVadContext> {
    let path = model_path(&VAD);
    if !is_present(&VAD) {
        tracing::warn!("VAD model missing; recognising without speech gating");
        return None;
    }
    // The VAD loader only takes a path: use the 8.3 form when the real one
    // is not plain ASCII (see `Recognizer::load_with`).
    let p = ascii_path(&path)?;
    let mut params = WhisperVadContextParams::new();
    params.set_n_threads(threads.min(4));
    match WhisperVadContext::new(&p, params) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("VAD model not loaded: {e:?}");
            None
        }
    }
}

fn ascii_path(path: &std::path::Path) -> Option<String> {
    let s = path.to_str()?;
    if s.is_ascii() {
        return Some(s.to_string());
    }
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide = HSTRING::from(path.as_os_str());
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetShortPathNameW(&wide, Some(&mut buf)) } as usize;
    let short = String::from_utf16(buf.get(..n)?).ok()?;
    (n > 0 && short.is_ascii()).then_some(short)
}

pub fn is_hallucination(text: &str) -> bool {
    let t = text.to_lowercase();
    let letters = t.chars().filter(|c| c.is_alphanumeric()).count();
    letters == 0 || HALLUCINATIONS.iter().any(|h| t.contains(h))
}

impl Recognizer {
    pub fn load(path: PathBuf) -> AppResult<Recognizer> {
        // Flash attention measured slower on the CPU (3.5 s vs 2.9 s, small).
        Self::load_with(path, false)
    }

    pub fn load_with(path: PathBuf, flash_attn: bool) -> AppResult<Recognizer> {
        whisper_rs::install_logging_hooks();
        let t = Instant::now();
        let mut params = WhisperContextParameters::default();
        params.flash_attn = flash_attn;
        // Read the file ourselves: whisper.cpp opens paths in the ANSI code
        // page, which breaks on profile folders with Cyrillic names.
        let bytes = std::fs::read(&path).map_err(|e| AppError::Storage(format!("Не удалось прочитать модель распознавания: {e}")))?;
        let ctx = WhisperContext::new_from_buffer_with_params(&bytes, params)
            .map_err(|e| AppError::platform(format!("Не удалось загрузить модель распознавания: {e}")))?;
        drop(bytes);
        let state = ctx.create_state().map_err(|e| AppError::platform(format!("Распознавание: {e}")))?;
        // Physical cores do the work; hyper-threads add little to whisper.cpp.
        let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let threads = (logical / 2).clamp(2, 8) as i32;
        let vad = load_vad(threads);
        tracing::info!(ms = t.elapsed().as_millis() as u64, threads, vad = vad.is_some(), model = %path.display(), "whisper model loaded");
        Ok(Recognizer { path, state, vad, threads, audio_ctx: 0 })
    }

    /// Diagnostics: time the mel spectrogram and the encoder separately.
    pub fn bench_encode(&mut self, pcm: &[f32]) -> AppResult<(u64, u64)> {
        let t = Instant::now();
        self.state.pcm_to_mel(pcm, self.threads as usize).map_err(|e| AppError::platform(e.to_string()))?;
        let mel = t.elapsed().as_millis() as u64;
        let t = Instant::now();
        self.state.encode(0, self.threads as usize).map_err(|e| AppError::platform(e.to_string()))?;
        Ok((mel, t.elapsed().as_millis() as u64))
    }

    /// Speech duration in `pcm` according to the VAD model.
    fn speech_ms(&mut self, pcm: &[f32]) -> Option<u64> {
        let vad = self.vad.as_mut()?;
        vad.detect_speech(pcm).ok()?;
        let probs = vad.probabilities();
        if probs.is_empty() {
            return Some(0);
        }
        let window_ms = pcm.len() as f64 / 16.0 / probs.len() as f64;
        Some((probs.iter().filter(|p| **p > 0.5).count() as f64 * window_ms) as u64)
    }

    /// Transcribe mono 16 kHz audio in Russian. Audio without speech (per
    /// the VAD model) returns an empty text without running whisper.
    pub fn transcribe(&mut self, pcm: &[f32]) -> AppResult<Transcript> {
        let t = Instant::now();
        let speech_ms = self.speech_ms(pcm);
        if speech_ms.is_some_and(|ms| ms < MIN_SPEECH_MS) {
            return Ok(Transcript { text: String::new(), no_speech: 1.0, speech_ms, ms: t.elapsed().as_millis() as u64 });
        }
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some("ru"));
        p.set_n_threads(self.threads);
        let secs = pcm.len() as f32 / 16_000.0;
        p.set_audio_ctx(if self.audio_ctx > 0 { self.audio_ctx } else { adaptive_ctx(secs) });
        // Guard against repetition loops: ~8 tokens/s of Russian speech.
        p.set_max_tokens(((secs * 12.0) as i32 + 24).min(224));
        p.set_no_context(true);
        p.set_no_timestamps(true);
        p.set_single_segment(true);
        p.set_suppress_blank(true);
        p.set_suppress_nst(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        // whisper.cpp needs at least 1 s; pad short commands with silence.
        let mut audio = pcm.to_vec();
        if audio.len() < 16_000 + 1_600 {
            audio.resize(16_000 + 1_600, 0.0);
        }
        self.state.full(p, &audio).map_err(|e| AppError::platform(format!("Ошибка распознавания: {e}")))?;
        let mut text = String::new();
        let mut no_speech = 0.0f32;
        for seg in self.state.as_iter() {
            no_speech = no_speech.max(seg.no_speech_probability());
            if let Ok(s) = seg.to_str_lossy() {
                text.push_str(&s);
            }
        }
        Ok(Transcript { text: text.trim().to_string(), no_speech, speech_ms, ms: t.elapsed().as_millis() as u64 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hallucination_filter() {
        assert!(is_hallucination("Продолжение следует..."));
        assert!(is_hallucination("Субтитры сделал DimaTorzok"));
        assert!(is_hallucination(" ... "));
        assert!(!is_hallucination("Открой калькулятор"));
        assert!(!is_hallucination("Сделай громкость 30%"));
    }

    #[test]
    fn encoder_window_covers_the_utterance() {
        assert_eq!(adaptive_ctx(1.0), 512);
        assert_eq!(adaptive_ctx(8.0), 512);
        assert_eq!(adaptive_ctx(12.0), 768);
        assert!(adaptive_ctx(20.0) as f32 >= 22.0 * 50.0);
        assert_eq!(adaptive_ctx(40.0), 1500);
    }

    #[test]
    fn catalog_lookup() {
        assert_eq!(model("small").file, "ggml-small-q8_0.bin");
        assert_eq!(model("unknown").id, "turbo");
        assert!(MODELS.iter().all(|m| m.sha256.len() == 64));
    }
}
