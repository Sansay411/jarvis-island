//! Neural speech synthesis through sherpa-onnx: Supertonic 3 (multi-speaker,
//! 31 languages incl. Russian) and Piper/VITS Russian voices.
//!
//! Voice ids: `supertonic-3:<speaker>` or a Piper asset id (`piper-ruslan`).

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsModelConfig, OfflineTtsSupertonicModelConfig,
    OfflineTtsVitsModelConfig,
};

use super::models::{self, find_file, native_path};
use crate::error::{AppError, AppResult};

pub struct NeuralVoice {
    tts: OfflineTts,
    gen: GenerationConfig,
    pub name: String,
}

fn file(dir: &Path, prefix: &str, ext: &str) -> AppResult<Option<String>> {
    match find_file(dir, prefix, ext) {
        Some(p) => Ok(Some(native_path(&p)?)),
        None => Err(AppError::Storage(format!("В папке голоса нет файла {prefix}{ext} — скачайте голос заново"))),
    }
}

fn exact(dir: &Path, name: &str) -> AppResult<Option<String>> {
    let p = dir.join(name);
    if p.exists() {
        Ok(Some(native_path(&p)?))
    } else {
        Err(AppError::Storage(format!("В папке голоса нет {name} — скачайте голос заново")))
    }
}

/// 4 threads on a 16-thread hybrid CPU beat 6 (efficiency cores slow it).
fn threads() -> i32 {
    let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (logical / 4).clamp(1, 4) as i32
}

impl NeuralVoice {
    /// `rate`: speaking-rate multiplier (1.0 = model default).
    pub fn load(voice: &str, rate: f32) -> AppResult<NeuralVoice> {
        let t = Instant::now();
        let (asset_id, speaker) = match voice.split_once(':') {
            Some((a, s)) => (a, s.parse::<i32>().unwrap_or(0)),
            None => (voice, 0),
        };
        let asset = models::asset(asset_id)
            .filter(|a| a.kind == models::Kind::Tts)
            .ok_or_else(|| AppError::invalid(format!("Неизвестный голос: {voice}")))?;
        if !asset.installed() {
            return Err(AppError::invalid(format!("Голос «{}» не скачан — откройте Настройки → Голос", asset.label)));
        }
        let dir = asset.path();
        let mut model = OfflineTtsModelConfig { num_threads: threads(), provider: Some("cpu".into()), ..Default::default() };
        let mut gen = GenerationConfig { speed: rate.clamp(0.5, 2.0), sid: speaker, ..Default::default() };
        if asset_id.starts_with("supertonic") {
            model.supertonic = OfflineTtsSupertonicModelConfig {
                duration_predictor: file(&dir, "duration_predictor", ".onnx")?,
                text_encoder: file(&dir, "text_encoder", ".onnx")?,
                vector_estimator: file(&dir, "vector_estimator", ".onnx")?,
                vocoder: file(&dir, "vocoder", ".onnx")?,
                tts_json: exact(&dir, "tts.json")?,
                unicode_indexer: exact(&dir, "unicode_indexer.bin")?,
                voice_style: exact(&dir, "voice.bin")?,
            };
            let mut extra = HashMap::new();
            extra.insert("lang".to_string(), serde_json::json!("ru"));
            gen.extra = Some(extra);
            // Flow-matching steps: quality over ~0.3 s of latency. Measured on a
            // 3 s phrase: 8 → 950 ms, 5 → 657 ms; 3 shifts the pitch.
            gen.num_steps = 8;
        } else {
            model.vits = OfflineTtsVitsModelConfig {
                model: file(&dir, "ru_RU", ".onnx")?,
                tokens: exact(&dir, "tokens.txt")?,
                data_dir: exact(&dir, "espeak-ng-data")?,
                ..Default::default()
            };
        }
        let cfg = OfflineTtsConfig { model, max_num_sentences: 1, ..Default::default() };
        let tts = OfflineTts::create(&cfg).ok_or_else(|| AppError::platform("Не удалось загрузить нейросетевой голос"))?;
        if speaker >= tts.num_speakers().max(1) {
            gen.sid = 0;
        }
        let name = if asset_id.starts_with("supertonic") { format!("{} — голос {}", asset.label, gen.sid + 1) } else { asset.label.to_string() };
        tracing::info!(ms = t.elapsed().as_millis() as u64, voice, speakers = tts.num_speakers(), rate = tts.sample_rate(), "neural voice loaded");
        Ok(NeuralVoice { tts, gen, name })
    }

    pub fn num_speakers(&self) -> i32 {
        self.tts.num_speakers()
    }

    /// Mono float samples and their rate.
    pub fn synthesize(&self, text: &str) -> AppResult<(u32, Vec<f32>)> {
        let audio = self
            .tts
            .generate_with_config(text, &self.gen, None::<fn(&[f32], f32) -> bool>)
            .ok_or_else(|| AppError::platform("Нейросетевой голос не смог озвучить текст"))?;
        Ok((audio.sample_rate().max(1) as u32, audio.samples().to_vec()))
    }
}
