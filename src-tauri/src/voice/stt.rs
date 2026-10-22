//! Local speech recognition: GigaAM v3 (Sber; NeMo RNN-T trained on Russian
//! speech, with punctuation) through sherpa-onnx on the CPU.
//!
//! Whisper small, the first engine, garbled live Russian commands («Настой,
//! пожалуйста, браузер» for «Открой…»); GigaAM is built for Russian
//! voice-assistant speech and is several times faster on the CPU.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use super::models::{self, find_file, native_path};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub text: String,
    pub ms: u64,
}

pub struct Recognizer {
    pub dir: PathBuf,
    rec: OfflineRecognizer,
}

fn need(p: Option<PathBuf>, what: &str) -> AppResult<String> {
    let p = p.ok_or_else(|| AppError::Storage(format!("В папке модели распознавания нет файла {what} — скачайте модель заново")))?;
    native_path(&p)
}

impl Recognizer {
    /// `dir`: the unpacked GigaAM model folder.
    pub fn load(dir: &Path) -> AppResult<Recognizer> {
        let t = Instant::now();
        let mut cfg = OfflineRecognizerConfig::default();
        cfg.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(need(find_file(dir, "encoder", ".onnx"), "encoder")?),
            decoder: Some(need(find_file(dir, "decoder", ".onnx"), "decoder")?),
            joiner: Some(need(find_file(dir, "joiner", ".onnx"), "joiner")?),
        };
        cfg.model_config.tokens = Some(need(Some(dir.join("tokens.txt")).filter(|p| p.is_file()), "tokens.txt")?);
        let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        cfg.model_config.num_threads = (logical / 2).clamp(2, 6) as i32;
        cfg.model_config.provider = Some("cpu".into());
        cfg.decoding_method = Some("greedy_search".into());
        let rec = OfflineRecognizer::create(&cfg)
            .ok_or_else(|| AppError::platform("Не удалось загрузить модель распознавания речи"))?;
        tracing::info!(ms = t.elapsed().as_millis() as u64, threads = cfg.model_config.num_threads, model = %dir.display(), "speech recogniser loaded");
        Ok(Recognizer { dir: dir.to_path_buf(), rec })
    }

    /// Load the default model from the models folder.
    pub fn load_default() -> AppResult<Recognizer> {
        let a = models::asset("gigaam-v3").expect("catalog");
        Self::load(&a.path())
    }

    /// Transcribe mono 16 kHz audio.
    pub fn transcribe(&self, pcm: &[f32]) -> AppResult<Transcript> {
        self.transcribe_at(16_000, pcm)
    }

    /// Transcribe mono audio at any rate (sherpa-onnx resamples).
    pub fn transcribe_at(&self, rate: i32, pcm: &[f32]) -> AppResult<Transcript> {
        let t = Instant::now();
        let stream = self.rec.create_stream();
        stream.accept_waveform(rate, pcm);
        self.rec.decode(&stream);
        let text = stream.get_result().map(|r| r.text).unwrap_or_default();
        Ok(Transcript { text: text.trim().to_string(), ms: t.elapsed().as_millis() as u64 })
    }
}

/// Recognised text worth sending to the agent: has letters, is not a lone
/// filler sound.
pub fn is_meaningful(text: &str) -> bool {
    let letters: String = text.chars().filter(|c| c.is_alphabetic()).collect::<String>().to_lowercase();
    !letters.is_empty() && !matches!(letters.as_str(), "а" | "э" | "м" | "мм" | "ммм" | "эм" | "хм" | "угу" | "ага")
}

#[cfg(test)]
mod tests {
    use super::is_meaningful;

    #[test]
    fn filters_fillers() {
        assert!(is_meaningful("Открой браузер."));
        assert!(is_meaningful("Стоп!"));
        assert!(!is_meaningful(" ... "));
        assert!(!is_meaningful("Мм."));
    }
}
