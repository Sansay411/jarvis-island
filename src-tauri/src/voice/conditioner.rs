//! Microphone conditioning: high-pass / DC removal, RNNoise noise suppression,
//! speech-gated AGC with soft peak limiting, utterance peak normalisation,
//! per-device learned gain helpers, and mic-test evaluation.
//!
//! Runs on the captured mono stream so every consumer (Silero VAD, wake
//! preview, quick commands, local GigaAM, cloud SpeechKit, level meters)
//! receives clean, well-levelled audio even on quiet or noisy microphones.

use std::collections::BTreeMap;

use nnnoiseless::DenoiseState;
use serde::Serialize;

use super::audio::Resampler;

/// Internal processing rate required by RNNoise (`nnnoiseless`).
pub const WORK_RATE: u32 = 48_000;
/// Output sample rate expected by Silero VAD, GigaAM and cloud STT.
pub const OUT_RATE: u32 = 16_000;
/// RNNoise frame size at 48 kHz (10 ms).
pub const RNNOISE_FRAME: usize = DenoiseState::FRAME_SIZE;
/// Target post-AGC active speech RMS level in dBFS.
pub const TARGET_RMS_DB: f32 = -20.0;
/// Minimum AGC / manual gain in dB.
pub const MIN_GAIN_DB: f32 = -6.0;
/// Maximum AGC / manual gain in dB.
pub const MAX_GAIN_DB: f32 = 30.0;
/// In `auto` mode, blend of denoised signal (`wet`) into the STT stream.
/// VAD, wake preview and level meters use 100% denoised (`wet`); STT gets a
/// delay-aligned mix to preserve fricatives/consonants for neural ASR while
/// suppressing stationary hiss and rumble.
pub const AUTO_STT_WET_MIX: f32 = 0.65;
/// SQLite `kv` key storing learned AGC gain per microphone name.
pub const MIC_GAINS_KEY: &str = "voice.mic_gains";

const MAX_SAVED_DEVICES: usize = 32;
const HIGHPASS_HZ: f32 = 90.0;
const LIMITER_KNEE: f32 = 0.70; // ~ -3.1 dBFS
const LIMITER_HEADROOM: f32 = 0.25; // asymptotic ceiling 0.95 (~ -0.45 dBFS)
const PEAK_NORM_TARGET: f32 = 0.7079458; // -3.0 dBFS
const PEAK_NORM_MAX_GAIN: f32 = 15.848932; // +24.0 dB
const PEAK_NORM_MIN_PEAK: f32 = 1e-4; // -80 dBFS: leave pure digital silence alone

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GainMode {
    Auto,
    Manual(f32),
}

impl GainMode {
    pub fn from_settings(mode: &str, manual_db: f32) -> Self {
        if mode.eq_ignore_ascii_case("manual") {
            let db = if manual_db.is_finite() { manual_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB) } else { 0.0 };
            GainMode::Manual(db)
        } else {
            GainMode::Auto
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GainMode::Auto => "auto",
            GainMode::Manual(_) => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseSuppression {
    Off,
    Auto,
    Strong,
}

impl NoiseSuppression {
    pub fn from_settings(mode: &str) -> Self {
        match mode.trim().to_ascii_lowercase().as_str() {
            "off" => NoiseSuppression::Off,
            "strong" => NoiseSuppression::Strong,
            _ => NoiseSuppression::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NoiseSuppression::Off => "off",
            NoiseSuppression::Auto => "auto",
            NoiseSuppression::Strong => "strong",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConditionerConfig {
    pub gain: GainMode,
    pub noise: NoiseSuppression,
}

impl Default for ConditionerConfig {
    fn default() -> Self {
        ConditionerConfig {
            gain: GainMode::Auto,
            noise: NoiseSuppression::Auto,
        }
    }
}

impl ConditionerConfig {
    pub fn from_voice_settings(v: &crate::store::settings::VoiceSettings) -> Self {
        ConditionerConfig {
            gain: GainMode::from_settings(&v.mic_gain_mode, v.mic_gain_db),
            noise: NoiseSuppression::from_settings(&v.noise_suppression),
        }
    }
}

/// Telemetry from the most recently processed conditioner frame.
#[derive(Debug, Clone, Copy)]
pub struct FrameTelemetry {
    /// Post-HPF, pre-AGC frame RMS in dBFS.
    pub raw_db: f32,
    /// Post-conditioning VAD-stream frame RMS in dBFS.
    pub post_db: f32,
    /// Learned pre-AGC noise floor in dBFS.
    pub raw_floor_db: f32,
    /// Learned post-conditioning noise floor in dBFS.
    pub post_floor_db: f32,
    /// Active gain in dB (AGC or manual).
    pub gain_db: f32,
    /// RNNoise speech probability (0..1) when denoiser is active, else 0.
    pub speech_prob: f32,
    /// Whether the AGC speech gate considered the last frame active speech.
    pub speech_active: bool,
}

impl Default for FrameTelemetry {
    fn default() -> Self {
        FrameTelemetry {
            raw_db: -90.0,
            post_db: -90.0,
            raw_floor_db: -70.0,
            post_floor_db: -70.0,
            gain_db: 0.0,
            speech_prob: 0.0,
            speech_active: false,
        }
    }
}

pub fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

pub fn rms_to_db(rms: f32) -> f32 {
    20.0 * rms.max(1e-6).log10()
}

pub fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Smooth soft peak limiter: unity below -3.1 dBFS, tanh knee above,
/// hard ceiling below full scale (`|y| < 1.0`).
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= LIMITER_KNEE {
        return x;
    }
    let over = (a - LIMITER_KNEE) / LIMITER_HEADROOM;
    let y = LIMITER_KNEE + LIMITER_HEADROOM * over.tanh();
    x.signum() * y.min(0.98)
}

/// Peak-normalise an utterance to roughly -3 dBFS (boost capped at +24 dB)
/// right before STT so quiet or early-convergence phrases read reliably.
pub fn peak_normalize(pcm: &[f32]) -> Vec<f32> {
    if pcm.is_empty() {
        return Vec::new();
    }
    let peak = pcm.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if !peak.is_finite() || peak < PEAK_NORM_MIN_PEAK {
        return pcm.to_vec();
    }
    // Already well-levelled (-6 dBFS .. -1 dBFS): skip unnecessary rescaling.
    if (0.50..=0.89).contains(&peak) {
        return pcm.to_vec();
    }
    let gain = (PEAK_NORM_TARGET / peak).clamp(0.70, PEAK_NORM_MAX_GAIN);
    pcm.iter().map(|&s| (s * gain).clamp(-0.98, 0.98)).collect()
}

/// Single Direct Form II Transposed biquad section.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn highpass(sample_rate: f32, cutoff_hz: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * (cutoff_hz as f64 / sample_rate as f64);
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q as f64);
        let a0 = 1.0 + alpha;
        let b0 = ((1.0 + cos_w0) * 0.5) / a0;
        let b1 = -(1.0 + cos_w0) / a0;
        let b2 = ((1.0 + cos_w0) * 0.5) / a0;
        let a1 = (-2.0 * cos_w0) / a0;
        let a2 = (1.0 - alpha) / a0;
        Biquad {
            b0: b0 as f32,
            b1: b1 as f32,
            b2: b2 as f32,
            a1: a1 as f32,
            a2: a2 as f32,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// 4th-order Butterworth high-pass filter (~90 Hz): removes DC offset,
/// 50/60 Hz mains hum (>20 dB down at 50 Hz) and desk rumble while leaving
/// voice fundamentals above 115 Hz intact.
#[derive(Debug, Clone, Copy)]
pub struct HighPass {
    s1: Biquad,
    s2: Biquad,
}

impl HighPass {
    pub fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        let sr = sample_rate.max(8_000) as f32;
        let fc = cutoff_hz.clamp(20.0, sr * 0.25);
        // 4th-order Butterworth pole Q factors.
        HighPass {
            s1: Biquad::highpass(sr, fc, 0.541_196_1),
            s2: Biquad::highpass(sr, fc, 1.306_563),
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        self.s2.tick(self.s1.tick(x))
    }

    pub fn process_in_place(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.tick(*s);
        }
    }

    pub fn reset(&mut self) {
        self.s1.reset();
        self.s2.reset();
    }
}

/// Exact 3:1 low-pass FIR decimator (48 kHz -> 16 kHz) with an 11-tap
/// Blackman-windowed sinc kernel (~0.10 ms group delay, >55 dB alias
/// rejection above 9 kHz, exact 480 -> 160 sample framing).
struct Decimator3 {
    taps: [f32; 11],
    hist: [f32; 11],
    idx: usize,
    phase: usize,
}

impl Decimator3 {
    fn new() -> Self {
        const N: usize = 11;
        let mid = (N - 1) as f64 / 2.0;
        // Cutoff ~6.8 kHz at 48 kHz (normalized 0.1417).
        let fc = 6_800.0 / WORK_RATE as f64;
        let mut raw = [0.0f64; N];
        let mut sum = 0.0;
        for (i, slot) in raw.iter_mut().enumerate() {
            let n = i as f64 - mid;
            let sinc = if n.abs() < 1e-9 {
                2.0 * fc
            } else {
                (2.0 * std::f64::consts::PI * fc * n).sin() / (std::f64::consts::PI * n)
            };
            let w = 0.42
                - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (N - 1) as f64).cos()
                + 0.08 * (4.0 * std::f64::consts::PI * i as f64 / (N - 1) as f64).cos();
            *slot = sinc * w;
            sum += *slot;
        }
        let mut taps = [0.0f32; N];
        for i in 0..N {
            taps[i] = (raw[i] / sum) as f32;
        }
        Decimator3 { taps, hist: [0.0; N], idx: 0, phase: 0 }
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        out.reserve(input.len() / 3 + 1);
        for &x in input {
            self.hist[self.idx] = x;
            self.idx = (self.idx + 1) % self.taps.len();
            self.phase += 1;
            if self.phase == 3 {
                self.phase = 0;
                let mut acc = 0.0f32;
                let mut p = self.idx;
                for &c in &self.taps {
                    acc += self.hist[p] * c;
                    p += 1;
                    if p == self.taps.len() {
                        p = 0;
                    }
                }
                out.push(acc);
            }
        }
    }

    fn reset(&mut self) {
        self.hist = [0.0; 11];
        self.idx = 0;
        self.phase = 0;
    }
}

/// Speech-gated feed-forward AGC with smooth attack/release and noise-floor
/// tracking. Operates on 10 ms frames.
#[derive(Debug, Clone)]
pub struct Agc {
    gain_db: f32,
    raw_floor_db: f32,
    raw_floor_seeded: bool,
    post_floor_db: f32,
    post_floor_seeded: bool,
    onset_frames: u32,
    learned_speech_frames: u32,
}

impl Agc {
    pub fn new(initial_gain_db: f32) -> Self {
        let g = if initial_gain_db.is_finite() {
            initial_gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB)
        } else {
            0.0
        };
        Agc {
            gain_db: g,
            raw_floor_db: -70.0,
            raw_floor_seeded: false,
            post_floor_db: -70.0,
            post_floor_seeded: false,
            onset_frames: 0,
            learned_speech_frames: 0,
        }
    }

    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    pub fn raw_floor_db(&self) -> f32 {
        self.raw_floor_db
    }

    pub fn post_floor_db(&self) -> f32 {
        self.post_floor_db
    }

    /// True once AGC has seen enough speech frames to have a meaningful
    /// learned gain worth persisting.
    pub fn has_learned(&self) -> bool {
        self.learned_speech_frames >= 25 // >= 0.25 s of speech
    }

    pub fn set_gain_db(&mut self, gain_db: f32) {
        if gain_db.is_finite() {
            self.gain_db = gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
        }
    }

    /// Update speech gate, noise floor and AGC gain for one 10 ms frame.
    /// Returns `true` when the frame was classified as active user speech.
    pub fn update(
        &mut self,
        mode: GainMode,
        raw_db: f32,
        dry_db: f32,
        post_db: f32,
        rnnoise_vad: Option<f32>,
        echo_active: bool,
    ) -> bool {
        // Seed raw noise floor immediately from the first non-zero frame so
        // quiet mics do not start with an artificially high floor.
        if !self.raw_floor_seeded && raw_db > -115.0 {
            self.raw_floor_db = raw_db.clamp(-95.0, -25.0);
            self.raw_floor_seeded = true;
        }
        if !self.post_floor_seeded && post_db > -115.0 {
            self.post_floor_db = post_db.clamp(-95.0, -20.0);
            self.post_floor_seeded = true;
        }

        // While JARVIS plays through speakers, freeze AGC so echo never pulls
        // gain down or pumps it up; only allow the floor estimate to drop.
        if echo_active {
            self.onset_frames = 0;
            if raw_db < self.raw_floor_db && raw_db > -115.0 {
                self.raw_floor_db = (self.raw_floor_db + (raw_db - self.raw_floor_db) * 0.10).clamp(-95.0, -25.0);
            }
            if let GainMode::Manual(db) = mode {
                self.gain_db = db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
            }
            return false;
        }

        let raw_snr = raw_db - self.raw_floor_db;
        let candidate = if raw_db <= -82.0 {
            false
        } else {
            match rnnoise_vad {
                Some(vad) => {
                    if dry_db >= -38.0 {
                        // Signal is well inside RNNoise's trained operating
                        // range: rely on its neural speech detector so steady
                        // fans/hiss never pump AGC.
                        (vad >= 0.45 && raw_snr >= 3.0) || vad >= 0.72
                    } else {
                        // Very quiet signal before AGC has lifted it into
                        // RNNoise's sweet spot: allow strong SNR above room
                        // noise (with minimal neural support or clean speech
                        // energy) to kick-start convergence.
                        (vad >= 0.20 && raw_snr >= 7.0) || (vad >= 0.03 && raw_snr >= 12.0) || raw_snr >= 15.0
                    }
                }
                None => raw_snr >= 9.0 && raw_db > -72.0,
            }
        };

        if candidate {
            self.onset_frames = self.onset_frames.saturating_add(1);
        } else {
            self.onset_frames = 0;
        }
        // Require 2 consecutive frames (20 ms) so isolated clicks/keystrokes
        // do not move the AGC gain.
        let speech_active = self.onset_frames >= 2;

        match mode {
            GainMode::Manual(db) => {
                self.gain_db = db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
            }
            GainMode::Auto => {
                if speech_active {
                    self.learned_speech_frames = self.learned_speech_frames.saturating_add(1);
                    let desired = (TARGET_RMS_DB - raw_db).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
                    // Fast attack (turn down quickly when voice gets loud,
                    // ~100 ms), smooth release (boost steadily without
                    // pumping, ~500 ms).
                    let alpha = if desired < self.gain_db { 0.095 } else { 0.020 };
                    self.gain_db = (self.gain_db + (desired - self.gain_db) * alpha).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
                }
            }
        }

        if !speech_active && !candidate {
            // Track noise floors only during silence / noise-only frames.
            if raw_db > -115.0 {
                let a = if raw_db < self.raw_floor_db {
                    0.15
                } else if raw_snr < 8.0 {
                    0.012
                } else {
                    0.005
                };
                self.raw_floor_db = (self.raw_floor_db + (raw_db - self.raw_floor_db) * a).clamp(-95.0, -25.0);
            }
            if post_db > -115.0 {
                let a = if post_db < self.post_floor_db { 0.15 } else { 0.018 };
                self.post_floor_db = (self.post_floor_db + (post_db - self.post_floor_db) * a).clamp(-95.0, -20.0);
            }
        }

        speech_active
    }
}

/// Complete real-time microphone conditioning stage.
///
/// Consumes mono `f32` samples at the capture device's native sample rate and
/// outputs conditioned 16 kHz streams:
/// - `vad_out`: fully denoised (in `auto`/`strong`) or HPF+AGC (in `off`),
///   used by Silero VAD, wake preview, level meters and realtime.
/// - `stt_out`: in `auto` mode, a delay-aligned gentle mix of denoised + dry
///   HPF+AGC audio for optimal neural ASR accuracy; in `off` and `strong`,
///   `stt_out` is left empty because `vad_out` is used for both.
pub struct Conditioner {
    in_rate: u32,
    cfg: ConditionerConfig,
    in_resampler: Option<Resampler>,
    hpf: HighPass,
    denoise: Option<Box<DenoiseState<'static>>>,
    agc: Agc,
    prev_gain_lin: f32,
    /// Pending 48 kHz post-HPF samples waiting to form a 480-sample frame.
    pending_48k: Vec<f32>,
    /// Previous dry (post-HPF, post-AGC) 480-sample frame for exact 1-frame
    /// delay alignment with RNNoise overlap-add synthesis.
    prev_dry: [f32; RNNOISE_FRAME],
    has_prev_dry: bool,
    decim_vad: Decimator3,
    decim_stt: Decimator3,
    resampled_buf: Vec<f32>,
    /// Last frame telemetry.
    telemetry: FrameTelemetry,
    /// Configurable wet mix for `auto` STT stream (defaults to `AUTO_STT_WET_MIX`).
    auto_wet_mix: f32,
}

impl Conditioner {
    pub fn new(in_rate: u32, cfg: ConditionerConfig, initial_gain_db: f32) -> Self {
        let start_gain = match cfg.gain {
            GainMode::Manual(db) => db.clamp(MIN_GAIN_DB, MAX_GAIN_DB),
            GainMode::Auto => initial_gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB),
        };
        let in_resampler = (in_rate != WORK_RATE).then(|| Resampler::new(in_rate, WORK_RATE));
        let denoise = (cfg.noise != NoiseSuppression::Off).then(DenoiseState::new);
        let mut telemetry = FrameTelemetry::default();
        telemetry.gain_db = start_gain;
        Conditioner {
            in_rate,
            cfg,
            in_resampler,
            hpf: HighPass::new(WORK_RATE, HIGHPASS_HZ),
            denoise,
            agc: Agc::new(start_gain),
            prev_gain_lin: db_to_lin(start_gain),
            pending_48k: Vec::with_capacity(RNNOISE_FRAME * 4),
            prev_dry: [0.0; RNNOISE_FRAME],
            has_prev_dry: false,
            decim_vad: Decimator3::new(),
            decim_stt: Decimator3::new(),
            resampled_buf: Vec::with_capacity(2048),
            telemetry,
            auto_wet_mix: AUTO_STT_WET_MIX,
        }
    }

    pub fn config(&self) -> ConditionerConfig {
        self.cfg
    }

    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    pub fn telemetry(&self) -> FrameTelemetry {
        self.telemetry
    }

    pub fn gain_db(&self) -> f32 {
        self.agc.gain_db()
    }

    pub fn has_learned_gain(&self) -> bool {
        matches!(self.cfg.gain, GainMode::Auto) && self.agc.has_learned()
    }

    /// Override the `auto` mode STT wet mix ratio (used in tests/benchmarks).
    pub fn set_auto_wet_mix(&mut self, wet_mix: f32) {
        self.auto_wet_mix = wet_mix.clamp(0.0, 1.0);
    }

    /// Live-update settings (e.g. when the user tweaks sensitivity or noise
    /// suppression in Settings) without reopening the audio device.
    pub fn set_config(&mut self, cfg: ConditionerConfig) {
        if self.cfg.noise != cfg.noise {
            if cfg.noise == NoiseSuppression::Off {
                self.denoise = None;
            } else if self.denoise.is_none() {
                self.denoise = Some(DenoiseState::new());
            }
            self.has_prev_dry = false;
            self.decim_stt.reset();
        }
        if let GainMode::Manual(db) = cfg.gain {
            self.agc.set_gain_db(db);
        }
        self.cfg = cfg;
    }

    /// Update the starting AGC gain (e.g. after a mic test calibrated it).
    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.agc.set_gain_db(gain_db);
        self.prev_gain_lin = db_to_lin(self.agc.gain_db());
        self.telemetry.gain_db = self.agc.gain_db();
    }

    /// True when `stt_out` carries a distinct gently-denoised stream.
    pub fn dual_stream(&self) -> bool {
        self.cfg.noise == NoiseSuppression::Auto
    }

    /// Feed mono capture samples at `in_rate`. Appends 16 kHz conditioned
    /// samples to `vad_out` (and `stt_out` when `noise == Auto`).
    pub fn process(
        &mut self,
        mono_in: &[f32],
        echo_active: bool,
        vad_out: &mut Vec<f32>,
        stt_out: &mut Vec<f32>,
    ) -> FrameTelemetry {
        if mono_in.is_empty() {
            return self.telemetry;
        }
        if let Some(rs) = &mut self.in_resampler {
            self.resampled_buf.clear();
            rs.process(mono_in, &mut self.resampled_buf);
            for &s in &self.resampled_buf {
                self.pending_48k.push(self.hpf.tick(s));
            }
        } else {
            self.pending_48k.reserve(mono_in.len());
            for &s in mono_in {
                self.pending_48k.push(self.hpf.tick(s));
            }
        }

        let mut consumed = 0usize;
        while self.pending_48k.len() - consumed >= RNNOISE_FRAME {
            let mut frame = [0.0f32; RNNOISE_FRAME];
            frame.copy_from_slice(&self.pending_48k[consumed..consumed + RNNOISE_FRAME]);
            consumed += RNNOISE_FRAME;
            self.process_frame_48k(&frame, echo_active, vad_out, stt_out);
        }
        if consumed > 0 {
            self.pending_48k.drain(..consumed);
        }

        self.telemetry
    }

    fn process_frame_48k(
        &mut self,
        raw_frame: &[f32; RNNOISE_FRAME],
        echo_active: bool,
        vad_out: &mut Vec<f32>,
        stt_out: &mut Vec<f32>,
    ) {
        let raw_db = rms_to_db(rms_of(raw_frame));
        let target_gain_lin = db_to_lin(self.agc.gain_db());
        let start_gain_lin = self.prev_gain_lin;
        let inv_n = 1.0 / RNNOISE_FRAME as f32;

        // Apply smoothly ramped AGC gain + soft peak limiter before RNNoise so
        // quiet mics enter RNNoise near -20 dBFS and loud mics never clip it.
        let mut dry = [0.0f32; RNNOISE_FRAME];
        for i in 0..RNNOISE_FRAME {
            let t = (i + 1) as f32 * inv_n;
            let g = start_gain_lin + (target_gain_lin - start_gain_lin) * t;
            dry[i] = soft_limit(raw_frame[i] * g);
        }
        self.prev_gain_lin = target_gain_lin;
        let dry_db = rms_to_db(rms_of(&dry));

        match self.cfg.noise {
            NoiseSuppression::Off => {
                let speech_active = self.agc.update(self.cfg.gain, raw_db, dry_db, dry_db, None, echo_active);
                self.decim_vad.process(&dry, vad_out);
                self.telemetry = FrameTelemetry {
                    raw_db,
                    post_db: dry_db,
                    raw_floor_db: self.agc.raw_floor_db(),
                    post_floor_db: self.agc.post_floor_db(),
                    gain_db: self.agc.gain_db(),
                    speech_prob: 0.0,
                    speech_active,
                };
            }
            NoiseSuppression::Auto | NoiseSuppression::Strong => {
                let denoise = self.denoise.get_or_insert_with(DenoiseState::new);
                let mut in_i16 = [0.0f32; RNNOISE_FRAME];
                let mut out_i16 = [0.0f32; RNNOISE_FRAME];
                for i in 0..RNNOISE_FRAME {
                    in_i16[i] = dry[i] * 32_768.0;
                }
                let vad_prob = denoise.process_frame(&mut out_i16, &in_i16);
                let mut wet = [0.0f32; RNNOISE_FRAME];
                for i in 0..RNNOISE_FRAME {
                    wet[i] = soft_limit(out_i16[i] / 32_768.0);
                }
                let post_db = rms_to_db(rms_of(&wet));
                let speech_active = self.agc.update(
                    self.cfg.gain,
                    raw_db,
                    dry_db,
                    post_db,
                    Some(vad_prob),
                    echo_active,
                );
                self.decim_vad.process(&wet, vad_out);

                if self.cfg.noise == NoiseSuppression::Auto {
                    let w = self.auto_wet_mix;
                    let d = 1.0 - w;
                    let mut mixed = [0.0f32; RNNOISE_FRAME];
                    if self.has_prev_dry {
                        for i in 0..RNNOISE_FRAME {
                            mixed[i] = soft_limit(w * wet[i] + d * self.prev_dry[i]);
                        }
                    } else {
                        mixed.copy_from_slice(&wet);
                    }
                    self.decim_stt.process(&mixed, stt_out);
                }
                self.prev_dry = dry;
                self.has_prev_dry = true;

                self.telemetry = FrameTelemetry {
                    raw_db,
                    post_db,
                    raw_floor_db: self.agc.raw_floor_db(),
                    post_floor_db: self.agc.post_floor_db(),
                    gain_db: self.agc.gain_db(),
                    speech_prob: vad_prob,
                    speech_active,
                };
            }
        }
    }
}

// ---------- per-device learned gain persistence ----------

pub type LearnedGains = BTreeMap<String, f32>;

pub fn parse_learned_gains(raw: Option<&str>) -> LearnedGains {
    let Some(s) = raw else { return BTreeMap::new() };
    let Ok(map) = serde_json::from_str::<BTreeMap<String, f32>>(s) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter(|(k, v)| !k.trim().is_empty() && v.is_finite())
        .map(|(k, v)| (k, v.clamp(MIN_GAIN_DB, MAX_GAIN_DB)))
        .collect()
}

pub fn serialize_learned_gains(map: &LearnedGains) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".into())
}

pub fn lookup_learned_gain(map: &LearnedGains, device_name: &str) -> f32 {
    map.get(device_name.trim())
        .copied()
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(MIN_GAIN_DB, MAX_GAIN_DB))
        .unwrap_or(0.0)
}

/// Update `device_name`'s saved gain if it shifted by at least 0.5 dB.
/// Returns `true` when the map changed and should be written back.
pub fn update_learned_gain(map: &mut LearnedGains, device_name: &str, gain_db: f32) -> bool {
    let name = device_name.trim();
    if name.is_empty() || !gain_db.is_finite() {
        return false;
    }
    let rounded = (gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB) * 2.0).round() / 2.0;
    if let Some(&prev) = map.get(name) {
        if (prev - rounded).abs() < 0.5 {
            return false;
        }
    }
    if map.len() >= MAX_SAVED_DEVICES && !map.contains_key(name) {
        if let Some(first) = map.keys().next().cloned() {
            map.remove(&first);
        }
    }
    map.insert(name.to_string(), rounded);
    true
}

// ---------- guided microphone test evaluation ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicTestResult {
    pub device_name: String,
    pub noise_floor_db: f32,
    pub post_floor_db: f32,
    pub speech_level_db: f32,
    pub post_speech_db: f32,
    pub snr_db: f32,
    pub applied_gain_db: f32,
    pub recommended_gain_db: f32,
    pub transcript: String,
    pub wake_detected: bool,
    pub stt_available: bool,
    pub advice: String,
    /// "ok" | "warn" | "error"
    pub severity: String,
}

#[derive(Debug, Clone)]
pub struct MicTestInput {
    pub device_name: String,
    pub silence_raw_dbs: Vec<f32>,
    pub silence_post_dbs: Vec<f32>,
    pub speech_raw_dbs: Vec<f32>,
    pub speech_post_dbs: Vec<f32>,
    pub raw_peak: f32,
    pub config: ConditionerConfig,
    pub agc_gain_db: f32,
    pub transcript: String,
    pub stt_available: bool,
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return -90.0;
    }
    let idx = ((sorted.len() - 1) as f32 * p.clamp(0.0, 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

fn fmt_signed_db(db: f32) -> String {
    let r = db.round() as i32;
    if r >= 0 { format!("+{r} дБ") } else { format!("{r} дБ") }
}

/// Evaluate the captured silence + speech phases of the guided microphone test
/// and produce numeric metrics plus clear Russian advice.
pub fn evaluate_mic_test(input: &MicTestInput) -> MicTestResult {
    let mut sil_raw: Vec<f32> = input.silence_raw_dbs.iter().copied().filter(|v| v.is_finite()).collect();
    sil_raw.sort_by(|a, b| a.total_cmp(b));
    let mut sil_post: Vec<f32> = input.silence_post_dbs.iter().copied().filter(|v| v.is_finite()).collect();
    sil_post.sort_by(|a, b| a.total_cmp(b));

    let noise_floor_db = percentile(&sil_raw, 0.50).clamp(-95.0, 0.0);
    let post_floor_db = percentile(&sil_post, 0.50).clamp(-95.0, 0.0);

    // Measure active speech from the loudest frames above room noise (top ~25%
    // of the 4 s speaking window corresponds to ~1 s of spoken vowels/words).
    let mut sp_raw: Vec<f32> = input.speech_raw_dbs.iter().copied().filter(|v| v.is_finite()).collect();
    sp_raw.sort_by(|a, b| a.total_cmp(b));
    let mut sp_post: Vec<f32> = input.speech_post_dbs.iter().copied().filter(|v| v.is_finite()).collect();
    sp_post.sort_by(|a, b| a.total_cmp(b));

    let active_raw: Vec<f32> = sp_raw.iter().copied().filter(|&db| db >= noise_floor_db + 4.0).collect();
    let speech_level_db = if active_raw.len() >= 10 {
        percentile(&active_raw, 0.70)
    } else {
        percentile(&sp_raw, 0.85)
    }
    .clamp(-95.0, 0.0);

    let active_post: Vec<f32> = sp_post.iter().copied().filter(|&db| db >= post_floor_db + 4.0).collect();
    let post_speech_db = if active_post.len() >= 10 {
        percentile(&active_post, 0.70)
    } else {
        percentile(&sp_post, 0.85)
    }
    .clamp(-95.0, 0.0);

    let snr_db = (speech_level_db - noise_floor_db).max(0.0);
    let recommended_gain_db = if speech_level_db <= -78.0 {
        0.0
    } else {
        (TARGET_RMS_DB - speech_level_db).clamp(MIN_GAIN_DB, MAX_GAIN_DB)
    };
    let applied_gain_db = match input.config.gain {
        GainMode::Auto => {
            if active_raw.len() >= 10 {
                recommended_gain_db
            } else {
                input.agc_gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB)
            }
        }
        GainMode::Manual(db) => db.clamp(MIN_GAIN_DB, MAX_GAIN_DB),
    };

    let transcript = input.transcript.trim().to_string();
    let wake_detected = super::wake::addressed(&transcript).is_some()
        || super::wake::from_name(&transcript, 4).is_some();

    let (severity, advice) = if speech_level_db <= -75.0 || (active_raw.len() < 8 && snr_db < 3.5) {
        (
            "error",
            "Микрофон почти ничего не слышит. Проверьте, не выключен ли он кнопкой или в параметрах звука Windows.".to_string(),
        )
    } else if snr_db < 10.0 && noise_floor_db > -48.0 {
        if input.config.noise != NoiseSuppression::Strong {
            (
                "warn",
                format!(
                    "Много шума (сигнал/шум {:.0} дБ) — включите сильное шумоподавление.",
                    snr_db
                ),
            )
        } else {
            (
                "warn",
                format!(
                    "Много фонового шума (сигнал/шум {:.0} дБ) — попробуйте говорить чуть ближе к микрофону.",
                    snr_db
                ),
            )
        }
    } else if noise_floor_db > -44.0 && input.config.noise == NoiseSuppression::Off {
        (
            "warn",
            format!(
                "Заметен фоновый шум ({:.0} дБ) — включите обычное или сильное шумоподавление.",
                noise_floor_db
            ),
        )
    } else if input.raw_peak >= 0.98 || speech_level_db > -8.0 {
        (
            "warn",
            "Микрофон слишком громкий — немного убавьте чувствительность или отодвиньте его.".to_string(),
        )
    } else if speech_level_db < -32.0 {
        match input.config.gain {
            GainMode::Auto => {
                let sev = if snr_db < 12.0 { "warn" } else { "ok" };
                let tail = if snr_db < 12.0 && input.config.noise != NoiseSuppression::Strong {
                    " При заметном шуме поможет сильное шумоподавление."
                } else {
                    ""
                };
                (
                    sev,
                    format!(
                        "Микрофон тихий — включил усиление {}.{tail}",
                        fmt_signed_db(applied_gain_db)
                    ),
                )
            }
            GainMode::Manual(manual) if manual + 4.0 < recommended_gain_db => (
                "warn",
                format!(
                    "Микрофон тихий ({:.0} дБ) — включите «Авто» или поставьте усиление около {}.",
                    speech_level_db,
                    fmt_signed_db(recommended_gain_db)
                ),
            ),
            GainMode::Manual(_) => (
                "ok",
                format!(
                    "Микрофон тихий, но с ручным усилением {} слышно хорошо.",
                    fmt_signed_db(applied_gain_db)
                ),
            ),
        }
    } else if input.stt_available && !wake_detected && transcript.is_empty() {
        (
            "warn",
            "Уровень звука нормальный, но речь не распозналась. Попробуйте повторить фразу чуть отчётливее.".to_string(),
        )
    } else if wake_detected {
        (
            "ok",
            "Отлично слышно — Джарвис уверенно услышал имя и фразу.".to_string(),
        )
    } else {
        (
            "ok",
            "Отлично слышно — уровни микрофона в норме.".to_string(),
        )
    };

    MicTestResult {
        device_name: input.device_name.clone(),
        noise_floor_db: round1(noise_floor_db),
        post_floor_db: round1(post_floor_db),
        speech_level_db: round1(speech_level_db),
        post_speech_db: round1(post_speech_db),
        snr_db: round1(snr_db),
        applied_gain_db: round1(applied_gain_db),
        recommended_gain_db: round1(recommended_gain_db),
        transcript,
        wake_detected,
        stt_available: input.stt_available,
        advice,
        severity: severity.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a harmonic speech-like signal (fundamental 150 Hz + formants
    /// 450, 900, 1800, 2600 Hz with syllable amplitude envelope) at `rate`.
    fn speech_like(rate: u32, seconds: f32, rms_db: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        let mut raw = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let f0 = 145.0 + 18.0 * (2.0 * std::f32::consts::PI * 2.3 * t).sin();
            let s = 0.45 * (2.0 * std::f32::consts::PI * f0 * t).sin()
                + 0.30 * (2.0 * std::f32::consts::PI * (f0 * 3.0) * t).sin()
                + 0.18 * (2.0 * std::f32::consts::PI * (f0 * 6.0) * t).sin()
                + 0.10 * (2.0 * std::f32::consts::PI * (f0 * 12.0) * t).sin()
                + 0.06 * (2.0 * std::f32::consts::PI * (f0 * 17.0) * t).sin();
            // Syllable modulation (4 Hz) keeping voice active most of the time.
            let env = 0.55 + 0.45 * (2.0 * std::f32::consts::PI * 4.0 * t).sin().abs();
            raw.push(s * env);
        }
        let cur = rms_of(&raw).max(1e-6);
        let scale = db_to_lin(rms_db) / cur;
        for s in &mut raw {
            *s *= scale;
        }
        raw
    }

    /// Deterministic PRNG white noise (xorshift32) at `rms_db`.
    fn white_noise(len: usize, rms_db: f32, seed: u32) -> Vec<f32> {
        let mut x = seed.max(1);
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let u = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
            out.push(u);
        }
        let cur = rms_of(&out).max(1e-6);
        let scale = db_to_lin(rms_db) / cur;
        for s in &mut out {
            *s *= scale;
        }
        out
    }

    #[test]
    fn highpass_removes_dc_and_50hz_hum_while_preserving_voice() {
        let sr = 48_000u32;
        let n = sr as usize; // 1 s

        // Pure DC offset (+0.4)
        let mut dc = vec![0.4f32; n];
        let mut hp = HighPass::new(sr, HIGHPASS_HZ);
        hp.process_in_place(&mut dc);
        let tail_rms = rms_of(&dc[n / 2..]);
        assert!(tail_rms < 1e-4, "DC not removed: tail_rms={tail_rms}");

        // 50 Hz mains hum
        let mut hum: Vec<f32> = (0..n)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 50.0 * i as f32 / sr as f32).sin())
            .collect();
        let in_hum_db = rms_to_db(rms_of(&hum[n / 2..]));
        hp.reset();
        hp.process_in_place(&mut hum);
        let out_hum_db = rms_to_db(rms_of(&hum[n / 2..]));
        let atten = in_hum_db - out_hum_db;
        assert!(atten > 18.0, "50 Hz hum only attenuated by {atten:.1} dB");

        // 400 Hz voice formant passes nearly untouched
        let mut voice: Vec<f32> = (0..n)
            .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 400.0 * i as f32 / sr as f32).sin())
            .collect();
        let in_v_db = rms_to_db(rms_of(&voice[n / 2..]));
        hp.reset();
        hp.process_in_place(&mut voice);
        let out_v_db = rms_to_db(rms_of(&voice[n / 2..]));
        assert!((in_v_db - out_v_db).abs() < 0.3, "voice attenuated: in={in_v_db:.2} out={out_v_db:.2}");
    }

    #[test]
    fn limiter_never_exceeds_full_scale_and_is_transparent_below_knee() {
        for &x in &[-0.65, -0.2, 0.0, 0.3, 0.69] {
            assert!((soft_limit(x) - x).abs() < 1e-6);
        }
        for &x in &[-100.0, -5.0, -1.5, -1.0, 1.0, 1.5, 5.0, 100.0] {
            let y = soft_limit(x);
            assert!(y.abs() < 1.0, "soft_limit({x}) = {y} exceeded full scale");
            assert_eq!(y.signum(), x.signum());
        }
        // Monotonic
        let mut prev = -1.0f32;
        for i in -200..=200 {
            let y = soft_limit(i as f32 * 0.05);
            assert!(y >= prev, "not monotonic at {i}: {y} < {prev}");
            prev = y;
        }
    }

    #[test]
    fn agc_converges_from_quiet_speech_and_holds_during_silence_and_noise() {
        let mut cond = Conditioner::new(48_000, ConditionerConfig::default(), 0.0);
        let mut vad_out = Vec::new();
        let mut stt_out = Vec::new();

        // 2 s of quiet room noise (-65 dBFS) — AGC must NOT raise gain!
        let room = white_noise(48_000 * 2, -65.0, 42);
        for chunk in room.chunks(480) {
            cond.process(chunk, false, &mut vad_out, &mut stt_out);
        }
        assert!(
            cond.gain_db().abs() < 0.5,
            "AGC pumped during noise-only silence: gain={:.2} dB",
            cond.gain_db()
        );

        // 3 s of quiet speech (-45 dBFS) over -65 dBFS room noise
        let sp = speech_like(48_000, 3.0, -45.0);
        let bg = white_noise(sp.len(), -65.0, 99);
        let quiet_speech: Vec<f32> = sp.iter().zip(&bg).map(|(s, n)| s + n).collect();
        vad_out.clear();
        stt_out.clear();
        for chunk in quiet_speech.chunks(480) {
            cond.process(chunk, false, &mut vad_out, &mut stt_out);
        }
        let gain_after_speech = cond.gain_db();
        assert!(
            gain_after_speech > 18.0 && gain_after_speech <= MAX_GAIN_DB,
            "AGC failed to converge on -45 dBFS speech: gain={gain_after_speech:.2} dB"
        );
        // Check last 1 s of conditioned STT audio is near -20 dBFS target
        let tail_db = rms_to_db(rms_of(&stt_out[stt_out.len() - 16_000..]));
        assert!(
            (tail_db - TARGET_RMS_DB).abs() < 6.0,
            "post-AGC speech not near target: tail_db={tail_db:.1} dB (gain={gain_after_speech:.1} dB)"
        );

        // Another 2 s of room noise — AGC must HOLD the learned gain, never pump higher!
        let more_noise = white_noise(48_000 * 2, -65.0, 777);
        for chunk in more_noise.chunks(480) {
            cond.process(chunk, false, &mut vad_out, &mut stt_out);
        }
        assert!(
            (cond.gain_db() - gain_after_speech).abs() < 0.5,
            "AGC drifted during post-speech silence: before={gain_after_speech:.2} after={:.2}",
            cond.gain_db()
        );

        // Loud burst (+0 dBFS) never clips output
        let loud = speech_like(48_000, 0.5, -2.0);
        vad_out.clear();
        stt_out.clear();
        for chunk in loud.chunks(480) {
            cond.process(chunk, false, &mut vad_out, &mut stt_out);
        }
        let peak_vad = vad_out.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        let peak_stt = stt_out.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak_vad < 1.0 && peak_stt < 1.0, "clipped: vad={peak_vad} stt={peak_stt}");
        assert!(cond.gain_db() < gain_after_speech - 5.0, "AGC did not back off on loud speech");
    }

    #[test]
    fn agc_does_not_pump_on_loud_steady_noise_alone() {
        let mut cond = Conditioner::new(48_000, ConditionerConfig::default(), 0.0);
        let mut vad_out = Vec::new();
        let mut stt_out = Vec::new();
        let steady = white_noise(48_000 * 3, -38.0, 1234);
        for chunk in steady.chunks(480) {
            cond.process(chunk, false, &mut vad_out, &mut stt_out);
        }
        assert!(
            cond.gain_db().abs() < 0.5,
            "AGC pumped on steady -38 dBFS noise: gain={:.2} dB",
            cond.gain_db()
        );
    }

    #[test]
    fn denoiser_framing_keeps_exact_sample_counts_across_rates_and_modes() {
        // 48 kHz -> 16 kHz: 48000 samples (1.0 s) must yield exact 16000 samples
        for mode in [NoiseSuppression::Off, NoiseSuppression::Auto, NoiseSuppression::Strong] {
            let mut cond = Conditioner::new(
                48_000,
                ConditionerConfig { gain: GainMode::Auto, noise: mode },
                0.0,
            );
            let input = speech_like(48_000, 1.0, -24.0);
            let mut vad = Vec::new();
            let mut stt = Vec::new();
            // Feed in uneven WASAPI-like callback chunks (e.g. 441 / 512 samples)
            for c in input.chunks(512) {
                cond.process(c, false, &mut vad, &mut stt);
            }
            assert_eq!(vad.len(), 16_000, "mode={mode:?} vad len");
            if mode == NoiseSuppression::Auto {
                assert_eq!(stt.len(), 16_000, "mode={mode:?} stt len");
            } else {
                assert!(stt.is_empty(), "mode={mode:?} should not allocate stt");
            }
        }

        // 44.1 kHz -> 48 kHz -> 16 kHz: 1.0 s should be within ~160 samples (one 10 ms frame)
        let mut cond44 = Conditioner::new(44_100, ConditionerConfig::default(), 0.0);
        let input44 = speech_like(44_100, 1.0, -24.0);
        let mut vad44 = Vec::new();
        let mut stt44 = Vec::new();
        for c in input44.chunks(441) {
            cond44.process(c, false, &mut vad44, &mut stt44);
        }
        assert!((vad44.len() as i64 - 16_000).abs() <= 160, "44.1k->16k len={}", vad44.len());
        assert_eq!(vad44.len(), stt44.len());

        // 16 kHz -> 48 kHz -> 16 kHz: 1.0 s within one frame
        let mut cond16 = Conditioner::new(16_000, ConditionerConfig::default(), 0.0);
        let input16 = speech_like(16_000, 1.0, -24.0);
        let mut vad16 = Vec::new();
        let mut stt16 = Vec::new();
        for c in input16.chunks(512) {
            cond16.process(c, false, &mut vad16, &mut stt16);
        }
        assert!((vad16.len() as i64 - 16_000).abs() <= 160, "16k->16k len={}", vad16.len());
        assert_eq!(vad16.len(), stt16.len());
    }

    #[test]
    fn rnnoise_suppresses_stationary_hiss_and_echo_freezes_agc() {
        let mut cond = Conditioner::new(48_000, ConditionerConfig::default(), 15.0);
        let mut vad_out = Vec::new();
        let mut stt_out = Vec::new();

        // Stationary noise at -45 dBFS + 15 dB gain = -30 dBFS entering RNNoise
        let hiss = white_noise(48_000, -45.0, 555);
        for c in hiss.chunks(480) {
            cond.process(c, false, &mut vad_out, &mut stt_out);
        }
        let post_noise_db = rms_to_db(rms_of(&vad_out[8_000..]));
        assert!(
            post_noise_db < -45.0,
            "RNNoise did not suppress stationary noise: post_noise_db={post_noise_db:.1} dB"
        );

        // Echo active: even loud speech-like audio must NOT move AGC gain
        let before = cond.gain_db();
        let echo_speech = speech_like(48_000, 1.0, -10.0);
        for c in echo_speech.chunks(480) {
            cond.process(c, true, &mut vad_out, &mut stt_out);
        }
        assert_eq!(cond.gain_db(), before, "AGC moved while echo_active=true");
    }

    #[test]
    fn peak_normalise_lifts_quiet_utterance_and_ignores_silence() {
        let quiet: Vec<f32> = (0..1600).map(|i| 0.02 * (i as f32 * 0.1).sin()).collect();
        let norm = peak_normalize(&quiet);
        let peak = norm.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!((peak - PEAK_NORM_TARGET).abs() < 0.05, "peak={peak}");

        // Pure silence stays silence
        let silent = vec![0.0f32; 1600];
        assert_eq!(peak_normalize(&silent), silent);

        // Boost is capped at +24 dB
        let tiny = vec![0.001f32; 1600];
        let capped = peak_normalize(&tiny);
        let capped_peak = capped.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!((capped_peak - 0.001 * PEAK_NORM_MAX_GAIN).abs() < 1e-4);
    }

    #[test]
    fn learned_gains_roundtrip_and_clamp() {
        let mut map = parse_learned_gains(None);
        assert_eq!(lookup_learned_gain(&map, "USB Mic"), 0.0);
        assert!(update_learned_gain(&mut map, "USB Mic", 22.3));
        assert!(!update_learned_gain(&mut map, "USB Mic", 22.4)); // < 0.5 dB diff
        assert!(update_learned_gain(&mut map, "Hot Mic", -20.0)); // clamped to -6
        let json = serialize_learned_gains(&map);
        let loaded = parse_learned_gains(Some(&json));
        assert_eq!(lookup_learned_gain(&loaded, "USB Mic"), 22.5);
        assert_eq!(lookup_learned_gain(&loaded, "Hot Mic"), MIN_GAIN_DB);
    }

    #[test]
    fn mic_test_evaluation_covers_quiet_noisy_muted_and_good_mics() {
        // Quiet mic in Auto mode
        let quiet = evaluate_mic_test(&MicTestInput {
            device_name: "Laptop Mic".into(),
            silence_raw_dbs: vec![-68.0; 100],
            silence_post_dbs: vec![-62.0; 100],
            speech_raw_dbs: vec![-42.0; 100],
            speech_post_dbs: vec![-20.0; 100],
            raw_peak: 0.04,
            config: ConditionerConfig::default(),
            agc_gain_db: 22.0,
            transcript: "Джарвис, какая сегодня погода?".into(),
            stt_available: true,
        });
        assert_eq!(quiet.severity, "ok");
        assert!(quiet.wake_detected);
        assert!(quiet.advice.contains("Микрофон тихий") && quiet.advice.contains("+22 дБ"), "{}", quiet.advice);

        // Noisy room
        let noisy = evaluate_mic_test(&MicTestInput {
            device_name: "Webcam".into(),
            silence_raw_dbs: vec![-38.0; 100],
            silence_post_dbs: vec![-50.0; 100],
            speech_raw_dbs: vec![-31.0; 100],
            speech_post_dbs: vec![-22.0; 100],
            raw_peak: 0.12,
            config: ConditionerConfig { gain: GainMode::Auto, noise: NoiseSuppression::Auto },
            agc_gain_db: 11.0,
            transcript: "погода".into(),
            stt_available: true,
        });
        assert_eq!(noisy.severity, "warn");
        assert!(noisy.advice.contains("сильное шумоподавление"), "{}", noisy.advice);

        // Muted mic
        let muted = evaluate_mic_test(&MicTestInput {
            device_name: "Muted".into(),
            silence_raw_dbs: vec![-95.0; 100],
            silence_post_dbs: vec![-95.0; 100],
            speech_raw_dbs: vec![-95.0; 100],
            speech_post_dbs: vec![-95.0; 100],
            raw_peak: 0.0,
            config: ConditionerConfig::default(),
            agc_gain_db: 0.0,
            transcript: String::new(),
            stt_available: true,
        });
        assert_eq!(muted.severity, "error");

        // Good mic
        let good = evaluate_mic_test(&MicTestInput {
            device_name: "USB Studio".into(),
            silence_raw_dbs: vec![-60.0; 100],
            silence_post_dbs: vec![-70.0; 100],
            speech_raw_dbs: vec![-21.0; 100],
            speech_post_dbs: vec![-20.0; 100],
            raw_peak: 0.35,
            config: ConditionerConfig::default(),
            agc_gain_db: 1.0,
            transcript: "Джарвис, какая сегодня погода?".into(),
            stt_available: true,
        });
        assert_eq!(good.severity, "ok");
        assert!(good.advice.contains("Отлично слышно"), "{}", good.advice);
    }
}
