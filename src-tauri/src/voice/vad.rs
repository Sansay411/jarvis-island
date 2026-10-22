//! Utterance detection: Silero VAD (sherpa-onnx, neural) over 16 kHz audio.
//!
//! Silero decides what is speech — noise, music and clicks are not. A pause
//! shorter than `END_SILENCE_S` stays inside one utterance, so «Открой,
//! пожалуйста, … Яндекс» is one request, not two. While JARVIS is talking
//! through speakers, speech only counts if it is clearly louder than the
//! room (`echo_guard`), and segments that never passed that check are
//! dropped — JARVIS must not answer its own voice.

use std::collections::VecDeque;
use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::models::native_path;
use crate::error::{AppError, AppResult};

const END_SILENCE_S: f32 = 0.9;
const MIN_SPEECH_S: f32 = 0.25;
const MAX_SPEECH_S: f32 = 20.0;
/// Extra loudness over the room needed while JARVIS speaks.
const ECHO_MARGIN_DB: f32 = 12.0;
/// Audio kept around each detected phrase: Silero's segment can start a
/// little late and clip a short word's onset — a lone «Джарвис» came out
/// unrecognised in the app while the same file read fine whole.
const PAD_BEFORE: usize = 16_000 * 35 / 100;
const PAD_AFTER: usize = 16_000 * 15 / 100;
/// Recent audio kept for the padding (longest phrase plus margin).
const HISTORY: usize = 16_000 * 25;

#[derive(Debug, PartialEq)]
pub enum VadEvent {
    /// Speech began (barge-in point).
    Started,
    /// Utterance finished: mono 16 kHz samples.
    Ended(Vec<f32>),
}

pub fn chunk_db(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return -120.0;
    }
    let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
    20.0 * rms.max(1e-6).log10()
}

/// Room noise floor: fast down, slow up, learned from non-speech chunks.
#[derive(Debug, Clone, Copy)]
pub struct Floor(pub f32);

impl Default for Floor {
    fn default() -> Self {
        Floor(-55.0)
    }
}

impl Floor {
    pub fn learn(&mut self, db: f32) {
        let a = if db < self.0 { 0.3 } else { 0.03 };
        self.0 = (self.0 + (db - self.0) * a).max(-75.0);
    }

    /// Whether a chunk is loud enough to be the user over JARVIS's voice.
    pub fn passes_echo_guard(&self, db: f32) -> bool {
        db >= self.0 + ECHO_MARGIN_DB && db > -45.0
    }
}

pub struct Vad {
    det: VoiceActivityDetector,
    floor: Floor,
    /// Silero currently hears speech.
    in_speech: bool,
    /// The current segment was accepted as the user's (Started emitted).
    accepted: bool,
    /// JARVIS is speaking: apply the echo guard.
    pub echo_guard: bool,
    /// Recent samples; `history[0]` is sample number `history_base`.
    history: VecDeque<f32>,
    history_base: usize,
}

impl Vad {
    pub fn new(model: &Path) -> AppResult<Vad> {
        let silero = SileroVadModelConfig {
            model: Some(native_path(model)?),
            threshold: 0.5,
            min_silence_duration: END_SILENCE_S,
            min_speech_duration: MIN_SPEECH_S,
            window_size: 512,
            max_speech_duration: MAX_SPEECH_S,
        };
        let cfg = VadModelConfig {
            silero_vad: silero,
            ten_vad: Default::default(),
            sample_rate: 16_000,
            num_threads: 1,
            provider: Some("cpu".into()),
            debug: false,
        };
        let det = VoiceActivityDetector::create(&cfg, MAX_SPEECH_S + 10.0)
            .ok_or_else(|| AppError::platform("Не удалось загрузить детектор речи"))?;
        Ok(Vad { det, floor: Floor::default(), in_speech: false, accepted: false, echo_guard: false, history: VecDeque::with_capacity(HISTORY), history_base: 0 })
    }

    pub fn floor_db(&self) -> f32 {
        self.floor.0
    }

    pub fn is_active(&self) -> bool {
        self.in_speech && self.accepted
    }

    pub fn push(&mut self, chunk: &[f32]) -> Vec<VadEvent> {
        let db = chunk_db(chunk);
        self.det.accept_waveform(chunk);
        self.history.extend(chunk.iter().copied());
        while self.history.len() > HISTORY {
            self.history.pop_front();
            self.history_base += 1;
        }
        let mut events = Vec::new();
        // Finished segments first: each closes the utterance it belongs to.
        while let Some(seg) = self.det.front() {
            if self.accepted {
                events.push(VadEvent::Ended(self.padded(seg.start().max(0) as usize, seg.samples())));
            }
            self.det.pop();
            self.accepted = false;
        }
        let detected = self.det.detected();
        if detected {
            if !self.accepted && (!self.echo_guard || self.floor.passes_echo_guard(db)) {
                self.accepted = true;
                events.push(VadEvent::Started);
            }
        } else {
            self.floor.learn(db);
        }
        self.in_speech = detected;
        events
    }

    /// The phrase with some audio before and after it, from the history;
    /// the bare segment if that part of the history is gone.
    fn padded(&self, start: usize, segment: &[f32]) -> Vec<f32> {
        let seen = self.history_base + self.history.len();
        let from = start.saturating_sub(PAD_BEFORE).max(self.history_base);
        let to = (start + segment.len() + PAD_AFTER).min(seen);
        if start < self.history_base || from >= to {
            return segment.to_vec();
        }
        self.history.range(from - self.history_base..to - self.history_base).copied().collect()
    }

    /// Forget a half-heard utterance.
    pub fn reset(&mut self) {
        self.det.reset();
        self.history.clear();
        self.history_base = 0;
        self.in_speech = false;
        self.accepted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_tracks_room_and_guards_echo() {
        let mut f = Floor::default();
        for _ in 0..40 {
            f.learn(-60.0);
        }
        assert!((f.0 + 60.0).abs() < 1.0);
        assert!(!f.passes_echo_guard(-52.0));
        assert!(f.passes_echo_guard(-30.0));
        // Never below the absolute minimum, even in a silent room.
        let mut quiet = Floor::default();
        for _ in 0..100 {
            quiet.learn(-110.0);
        }
        assert!(quiet.0 >= -75.0);
        assert!(!quiet.passes_echo_guard(-50.0));
    }

    #[test]
    fn db_of_known_levels() {
        assert!((chunk_db(&vec![0.1; 1600]) + 20.0).abs() < 0.01);
        assert_eq!(chunk_db(&[]), -120.0);
    }
}
