//! Energy-based voice activity detection over 100 ms chunks of 16 kHz audio.
//!
//! Tracks the room's noise floor (slow average of quiet chunks) and opens an
//! utterance when two consecutive chunks are clearly above it; closes after
//! `END_SILENCE` of quiet. Keeps a short pre-roll so the first syllable is
//! not clipped. Deliberately simple: whisper decides what was said, this only
//! decides when to ask it.

use std::collections::VecDeque;

const PRE_ROLL: usize = 3; // 300 ms
const START_CHUNKS: usize = 2; // 200 ms above threshold
const END_SILENCE: usize = 7; // 700 ms
const MAX_CHUNKS: usize = 200; // 20 s
const MIN_VOICED: usize = 3; // ≥300 ms of speech, else it was a click or cough
const FLOOR_MIN_DB: f32 = -70.0;

#[derive(Debug, PartialEq)]
pub enum VadEvent {
    /// Speech began (barge-in point).
    Started,
    /// Utterance finished: mono 16 kHz samples including pre-roll.
    Ended(Vec<f32>),
    /// Speech began but was too short to be a command.
    Discarded,
}

pub struct Vad {
    floor_db: f32,
    pre: VecDeque<Vec<f32>>,
    utterance: Vec<f32>,
    active: bool,
    above: usize,
    silence: usize,
    voiced: usize,
    chunks: usize,
    /// Extra margin while JARVIS is talking (speaker echo).
    pub margin_boost_db: f32,
}

pub fn chunk_db(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return -120.0;
    }
    let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
    20.0 * rms.max(1e-6).log10()
}

impl Default for Vad {
    fn default() -> Self {
        Vad {
            floor_db: -55.0,
            pre: VecDeque::with_capacity(PRE_ROLL + 1),
            utterance: Vec::new(),
            active: false,
            above: 0,
            silence: 0,
            voiced: 0,
            chunks: 0,
            margin_boost_db: 0.0,
        }
    }
}

impl Vad {
    pub fn floor_db(&self) -> f32 {
        self.floor_db
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    fn start_threshold(&self) -> f32 {
        (self.floor_db + 14.0 + self.margin_boost_db).max(-48.0)
    }

    fn keep_threshold(&self) -> f32 {
        (self.floor_db + 8.0 + self.margin_boost_db).max(-54.0)
    }

    pub fn push(&mut self, chunk: &[f32]) -> Option<VadEvent> {
        let db = chunk_db(chunk);
        if !self.active {
            if db >= self.start_threshold() {
                self.above += 1;
            } else {
                self.above = 0;
                // Learn the noise floor only from quiet chunks: fast down, slow up.
                let a = if db < self.floor_db { 0.3 } else { 0.03 };
                self.floor_db = (self.floor_db + (db - self.floor_db) * a).max(FLOOR_MIN_DB);
            }
            self.pre.push_back(chunk.to_vec());
            if self.pre.len() > PRE_ROLL + START_CHUNKS {
                self.pre.pop_front();
            }
            if self.above >= START_CHUNKS {
                self.active = true;
                self.utterance = self.pre.drain(..).flatten().collect();
                self.voiced = self.above;
                self.chunks = self.utterance.len() / chunk.len().max(1);
                self.silence = 0;
                self.above = 0;
                return Some(VadEvent::Started);
            }
            return None;
        }

        self.utterance.extend_from_slice(chunk);
        self.chunks += 1;
        if db >= self.keep_threshold() {
            self.silence = 0;
            self.voiced += 1;
        } else {
            self.silence += 1;
        }
        if self.silence >= END_SILENCE || self.chunks >= MAX_CHUNKS {
            self.active = false;
            self.silence = 0;
            let audio = std::mem::take(&mut self.utterance);
            return Some(if self.voiced >= MIN_VOICED { VadEvent::Ended(audio) } else { VadEvent::Discarded });
        }
        None
    }

    /// Forget a half-heard utterance (e.g. the session was interrupted).
    pub fn reset(&mut self) {
        self.active = false;
        self.utterance.clear();
        self.pre.clear();
        self.above = 0;
        self.silence = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amp: f32) -> Vec<f32> {
        (0..1600).map(|i| amp * ((i as f32) * 0.2).sin()).collect()
    }

    #[test]
    fn detects_utterance_with_preroll() {
        let mut v = Vad::default();
        for _ in 0..20 {
            assert_eq!(v.push(&tone(0.001)), None);
        }
        assert!(v.floor_db() < -55.0);
        assert_eq!(v.push(&tone(0.2)), None);
        assert_eq!(v.push(&tone(0.2)), Some(VadEvent::Started));
        for _ in 0..10 {
            assert_eq!(v.push(&tone(0.2)), None);
        }
        let mut ended = None;
        for _ in 0..END_SILENCE {
            ended = v.push(&tone(0.001));
        }
        match ended {
            Some(VadEvent::Ended(a)) => {
                // pre-roll (3) + 2 start + 10 speech + 7 silence chunks
                assert_eq!(a.len(), (PRE_ROLL + 2 + 10 + END_SILENCE) * 1600);
            }
            other => panic!("expected Ended, got {other:?}"),
        }
    }

    #[test]
    fn short_blip_is_discarded() {
        let mut v = Vad::default();
        for _ in 0..10 {
            v.push(&tone(0.001));
        }
        v.push(&tone(0.3));
        assert_eq!(v.push(&tone(0.3)), Some(VadEvent::Started));
        let mut last = None;
        for _ in 0..END_SILENCE {
            last = v.push(&tone(0.001));
        }
        assert_eq!(last, Some(VadEvent::Discarded));
    }

    #[test]
    fn echo_margin_raises_threshold() {
        let mut v = Vad::default();
        for _ in 0..10 {
            v.push(&tone(0.003));
        }
        v.margin_boost_db = 20.0;
        for _ in 0..5 {
            assert_eq!(v.push(&tone(0.05)), None);
        }
    }
}
