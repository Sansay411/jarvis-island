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

const END_SILENCE_S: f32 = 0.75;
/// A short command («дальше», «пауза», «стоп») is done after this much
/// silence: a second detector closes it early, and the caller acts at once
/// if the words are an exact command. Anything else still waits for the
/// pause-tolerant end above.
const QUICK_SILENCE_S: f32 = 0.28;
/// Only phrases this short are checked early.
const QUICK_MAX: usize = 16_000 * 2;
// Short «Стоп», «Джар» and the first word before a pause must survive.
const MIN_SPEECH_S: f32 = 0.12;
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
    /// Early local-only recognition while waiting for the wake name.
    Preview(Vec<f32>),
    /// The utterance so far, while awake: if its words stop changing, the
    /// user is done and the sound that keeps the detector open is just the
    /// room (music, a video, a fan).
    Progress(Vec<f32>),
    /// A short phrase followed by a short pause: maybe a whole command.
    Quick(Vec<f32>),
}

/// Progress decodes start this long into an utterance and repeat this often.
const PROGRESS_FROM: usize = 16_000 * 3 / 2;
const PROGRESS_EVERY: usize = 16_000 * 7 / 10;

pub fn chunk_db(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return -120.0;
    }
    let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
    20.0 * rms.max(1e-6).log10()
}

/// Room noise floor: fast down, slow up, learned from non-speech chunks.
/// Clamped to `FLOOR_MIN_DB..=FLOOR_MAX_DB` so quiet mics are never held back
/// by an artificially high default, while digital zero (-120 dB) stays sane.
const FLOOR_MIN_DB: f32 = -90.0;
const FLOOR_MAX_DB: f32 = -20.0;
/// Reject only near-zero digital quantization noise, never quiet speech.
const MIN_AUDIBLE_DB: f32 = -85.0;

#[derive(Debug, Clone, Copy)]
pub struct Floor {
    pub db: f32,
    seeded: bool,
}

impl Default for Floor {
    fn default() -> Self {
        Floor { db: -70.0, seeded: false }
    }
}

impl Floor {
    pub fn learn(&mut self, db: f32) {
        if !db.is_finite() {
            return;
        }
        let clamped = db.clamp(FLOOR_MIN_DB, FLOOR_MAX_DB);
        if !self.seeded {
            self.db = clamped;
            self.seeded = true;
            return;
        }
        let a = if clamped < self.db { 0.3 } else { 0.03 };
        self.db = (self.db + (clamped - self.db) * a).clamp(FLOOR_MIN_DB, FLOOR_MAX_DB);
    }

    /// Whether a chunk is loud enough over the learned room floor to be the
    /// user speaking over JARVIS's voice. Purely relative to the room floor
    /// (with only a digital-silence guard), so quiet microphones still work.
    pub fn passes_echo_guard(&self, db: f32) -> bool {
        db >= self.db + ECHO_MARGIN_DB && db > MIN_AUDIBLE_DB
    }
}

pub struct Vad {
    det: VoiceActivityDetector,
    /// The same model with a short end pause (`QUICK_SILENCE_S`).
    quick: VoiceActivityDetector,
    /// Send `Quick` for short phrases.
    pub quick_preview: bool,
    quick_sent: bool,
    floor: Floor,
    /// Silero currently hears speech.
    in_speech: bool,
    /// The current segment was accepted as the user's (Started emitted).
    accepted: bool,
    /// JARVIS is speaking: apply the echo guard.
    pub echo_guard: bool,
    pub wake_preview: bool,
    /// Awake: send `Progress` while an utterance runs.
    pub endpoint_preview: bool,
    utterance_start: usize,
    last_preview: usize,
    /// Recent VAD-stream (denoised) samples; `history[0]` is `history_base`.
    history: VecDeque<f32>,
    /// Parallel STT-stream (gently denoised mix in `auto`) samples, aligned
    /// sample-for-sample with `history`.
    stt_history: VecDeque<f32>,
    history_base: usize,
    /// Absolute sample index where the last `Ended` audio begins (padding
    /// included) — subtitles of transcribed files.
    pub last_segment_start: usize,
}

impl Vad {
    pub fn new(model: &Path) -> AppResult<Vad> {
        let detector = |silence: f32| -> AppResult<VoiceActivityDetector> {
            let silero = SileroVadModelConfig {
                model: Some(native_path(model)?),
                threshold: 0.5,
                min_silence_duration: silence,
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
            VoiceActivityDetector::create(&cfg, MAX_SPEECH_S + 10.0).ok_or_else(|| AppError::platform("Не удалось загрузить детектор речи"))
        };
        Ok(Vad {
            det: detector(END_SILENCE_S)?,
            quick: detector(QUICK_SILENCE_S)?,
            quick_preview: false,
            quick_sent: false,
            floor: Floor::default(),
            in_speech: false,
            accepted: false,
            echo_guard: false,
            wake_preview: false,
            endpoint_preview: false,
            utterance_start: 0,
            last_preview: 0,
            history: VecDeque::with_capacity(HISTORY),
            stt_history: VecDeque::with_capacity(HISTORY),
            history_base: 0,
            last_segment_start: 0,
        })
    }

    pub fn floor_db(&self) -> f32 {
        self.floor.db
    }

    pub fn is_active(&self) -> bool {
        self.in_speech && self.accepted
    }

    pub fn push(&mut self, chunk: &[f32]) -> Vec<VadEvent> {
        self.push_pair(chunk, chunk)
    }

    /// Feed aligned VAD (denoised) and STT (gently denoised) 16 kHz chunks.
    /// Detection decisions and `Preview` use `vad_chunk`; `Ended`, `Progress`
    /// and `Quick` return audio sliced from the parallel `stt_chunk` history.
    pub fn push_pair(&mut self, vad_chunk: &[f32], stt_chunk: &[f32]) -> Vec<VadEvent> {
        let stt_src = if stt_chunk.len() == vad_chunk.len() { stt_chunk } else { vad_chunk };
        let db = chunk_db(vad_chunk);
        self.det.accept_waveform(vad_chunk);
        self.quick.accept_waveform(vad_chunk);
        self.history.extend(vad_chunk.iter().copied());
        self.stt_history.extend(stt_src.iter().copied());
        while self.history.len() > HISTORY {
            self.history.pop_front();
            self.stt_history.pop_front();
            self.history_base += 1;
        }
        let mut events = Vec::new();
        // The short-pause detector: a short phrase that has just gone quiet.
        let mut quick_closed = false;
        while let Some(seg) = self.quick.front() {
            quick_closed |= seg.samples().len() <= QUICK_MAX;
            self.quick.pop();
        }
        let main_closed = self.det.front().is_some();
        // Finished segments first: each closes the utterance it belongs to.
        while let Some(seg) = self.det.front() {
            if self.accepted {
                let start = seg.start().max(0) as usize;
                self.last_segment_start = start.saturating_sub(PAD_BEFORE).max(self.history_base);
                events.push(VadEvent::Ended(self.padded_from(&self.stt_history, start, seg.samples())));
            }
            self.det.pop();
            self.accepted = false;
        }
        let detected = self.det.detected();
        let seen = self.history_base + self.history.len();
        if quick_closed && !main_closed && self.accepted && self.quick_preview && !self.quick_sent && !self.echo_guard
            && seen.saturating_sub(self.utterance_start) <= QUICK_MAX + (QUICK_SILENCE_S * 16_000.) as usize
        {
            self.quick_sent = true;
            events.push(VadEvent::Quick(self.utterance_so_far(&self.stt_history)));
        }
        if detected {
            if !self.accepted && (!self.echo_guard || self.floor.passes_echo_guard(db)) {
                self.accepted = true;
                self.quick_sent = false;
                self.utterance_start = (self.history_base+self.history.len()).saturating_sub((MIN_SPEECH_S*16_000.) as usize);
                self.last_preview = self.utterance_start;
                events.push(VadEvent::Started);
            }
            // Decode only a short prefix while waiting for the name. This
            // opens the listening UI before the user finishes the command;
            // actions still wait for the normal, pause-tolerant final VAD.
            let seen=self.history_base+self.history.len();
            let length=seen.saturating_sub(self.utterance_start);
            if self.accepted && self.wake_preview && !self.echo_guard && (8_000..=40_000).contains(&length) && seen.saturating_sub(self.last_preview)>=5_120 {
                let from=self.utterance_start.saturating_sub(PAD_BEFORE).max(self.history_base);
                events.push(VadEvent::Preview(self.history.range(from-self.history_base..).copied().collect()));
                self.last_preview=seen;
            }
            if self.accepted && self.endpoint_preview && !self.echo_guard && length >= PROGRESS_FROM && seen.saturating_sub(self.last_preview) >= PROGRESS_EVERY {
                events.push(VadEvent::Progress(self.utterance_so_far(&self.stt_history)));
                self.last_preview = seen;
            }
        } else {
            self.floor.learn(db);
        }
        self.in_speech = detected;
        events
    }

    /// The phrase with some audio before and after it, from `buf`;
    /// the bare segment if that part of the history is gone.
    fn padded_from(&self, buf: &VecDeque<f32>, start: usize, segment: &[f32]) -> Vec<f32> {
        let seen = self.history_base + buf.len();
        let from = start.saturating_sub(PAD_BEFORE).max(self.history_base);
        let to = (start + segment.len() + PAD_AFTER).min(seen);
        if start < self.history_base || from >= to {
            return segment.to_vec();
        }
        buf.range(from - self.history_base..to - self.history_base).copied().collect()
    }

    fn utterance_so_far(&self, buf: &VecDeque<f32>) -> Vec<f32> {
        let from = self.utterance_start.saturating_sub(PAD_BEFORE).max(self.history_base);
        buf.range(from - self.history_base..).copied().collect()
    }

    /// Forget a half-heard utterance.
    pub fn reset(&mut self) {
        self.det.reset();
        self.quick.reset();
        self.history.clear();
        self.stt_history.clear();
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
        assert!((f.db + 60.0).abs() < 1.0);
        assert!(!f.passes_echo_guard(-52.0));
        assert!(f.passes_echo_guard(-30.0));
        // Quiet mic at low absolute levels still passes when clearly above floor.
        let mut quiet_mic = Floor::default();
        for _ in 0..20 {
            quiet_mic.learn(-72.0);
        }
        assert!((quiet_mic.db + 72.0).abs() < 1.0);
        assert!(!quiet_mic.passes_echo_guard(-65.0));
        assert!(quiet_mic.passes_echo_guard(-55.0));
        assert!(quiet_mic.passes_echo_guard(-50.0));
        // Pure digital silence clamps floor at FLOOR_MIN_DB and rejects quantization noise.
        let mut silent = Floor::default();
        for _ in 0..100 {
            silent.learn(-110.0);
        }
        assert!(silent.db >= FLOOR_MIN_DB);
        assert!(!silent.passes_echo_guard(-86.0));
        assert!(silent.passes_echo_guard(-50.0));
    }

    #[test]
    fn db_of_known_levels() {
        assert!((chunk_db(&vec![0.1; 1600]) + 20.0).abs() < 0.01);
        assert_eq!(chunk_db(&[]), -120.0);
    }
}
