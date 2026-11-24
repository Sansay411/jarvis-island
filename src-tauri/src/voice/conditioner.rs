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
/// In `auto` mode, the share of denoised (`wet`) audio in the recognition
/// stream; VAD, wake preview and the meters hear 100 % wet. Measured with
/// `live_quiet_noisy_speech_is_still_understood` (two local voices, speech
/// 25 dB quieter, hiss at -62/-55/-50 dBFS): wet 0.35, 0.50 and 0.65 all
/// gave 88.3 % word recall and 23/24 names, fully denoised 85.6 % and 21/24
/// (the plain old path: 82.8 %, 17/24). 0.65 is the top of that plateau:
/// the most hiss removed (about 9 dB on the mix) for the cloud recogniser
/// without the losses of fully denoised audio.
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
    /// Post-HPF, pre-AGC RMS in dBFS.
    pub raw_db: f32,
    /// After gain and limiter, before noise suppression, in dBFS.
    pub dry_db: f32,
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
            dry_db: -90.0,
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

/// Exact 3:1 low-pass FIR decimator (48 kHz -> 16 kHz): a 63-tap
/// Blackman-windowed sinc with its cut-off at 7 kHz (flat to ~5 kHz, the
/// stop band from ~9 kHz, 0.65 ms group delay). Only every third output is
/// computed, so 480 input samples give exactly 160 output samples.
struct Decimator3 {
    taps: [f32; DECIM_TAPS],
    /// Ring of the last `DECIM_TAPS` inputs, stored twice so a window is
    /// always one contiguous slice.
    hist: [f32; DECIM_TAPS * 2],
    idx: usize,
    phase: usize,
}

const DECIM_TAPS: usize = 63;

impl Decimator3 {
    fn new() -> Self {
        let n = DECIM_TAPS;
        let mid = (n - 1) as f64 / 2.0;
        let fc = 7_000.0 / WORK_RATE as f64;
        let mut raw = [0.0f64; DECIM_TAPS];
        for (i, slot) in raw.iter_mut().enumerate() {
            let x = i as f64 - mid;
            let sinc = if x.abs() < 1e-9 {
                2.0 * fc
            } else {
                (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x)
            };
            let ph = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
            *slot = sinc * (0.42 - 0.5 * ph.cos() + 0.08 * (2.0 * ph).cos());
        }
        let sum: f64 = raw.iter().sum();
        let mut taps = [0.0f32; DECIM_TAPS];
        for (t, r) in taps.iter_mut().zip(raw.iter()) {
            *t = (r / sum) as f32;
        }
        Decimator3 { taps, hist: [0.0; DECIM_TAPS * 2], idx: 0, phase: 0 }
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &x in input {
            self.hist[self.idx] = x;
            self.hist[self.idx + DECIM_TAPS] = x;
            self.idx = (self.idx + 1) % DECIM_TAPS;
            self.phase += 1;
            if self.phase == 3 {
                self.phase = 0;
                // Oldest sample first: hist[idx..idx + N].
                let win = &self.hist[self.idx..self.idx + DECIM_TAPS];
                let y: f32 = win.iter().zip(self.taps.iter()).map(|(a, b)| a * b).sum();
                // The filter can ring a little over the limiter's ceiling.
                out.push(y.clamp(-0.99, 0.99));
            }
        }
    }

    fn reset(&mut self) {
        self.hist = [0.0; DECIM_TAPS * 2];
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
    /// Active speech energy (linear power, pre-gain), averaged over speech
    /// frames only. Power averaging is dominated by the vowels, so weak
    /// consonants and word tails barely move it.
    speech_power: f32,
}

/// Share of each speech frame in `speech_power` (~0.33 s of speech)...
const SPEECH_POWER_RATE: f32 = 0.03;
/// ...and over the first 0.6 s of speech of a session, while the start
/// level is only a guess (a new microphone converges within a second).
const SPEECH_POWER_RATE_FIRST: f32 = 0.10;
const FIRST_SPEECH_FRAMES: u32 = 60;

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
            // As if the speech the start gain was learned on were heard.
            speech_power: db_to_lin(2.0 * (TARGET_RMS_DB - g)),
        }
    }

    /// The level of the user's active speech before gain, dBFS.
    pub fn speech_level_db(&self) -> f32 {
        10.0 * self.speech_power.max(1e-12).log10()
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
            self.speech_power = db_to_lin(2.0 * (TARGET_RMS_DB - self.gain_db));
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
                        // Steady noise sits at its own floor (SNR ~0), so a
                        // little SNR is required even when RNNoise is sure.
                        (vad >= 0.45 && raw_snr >= 3.0) || (vad >= 0.72 && raw_snr >= 1.5)
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
        // Require 3 consecutive frames (30 ms) so isolated clicks/keystrokes
        // and single noisy frames do not move the AGC gain.
        let speech_active = self.onset_frames >= 3;

        match mode {
            GainMode::Manual(db) => {
                self.gain_db = db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
            }
            GainMode::Auto => {
                if speech_active {
                    self.learned_speech_frames = self.learned_speech_frames.saturating_add(1);
                    let power = db_to_lin(2.0 * raw_db);
                    let rate = if self.learned_speech_frames <= FIRST_SPEECH_FRAMES { SPEECH_POWER_RATE_FIRST } else { SPEECH_POWER_RATE };
                    self.speech_power += (power - self.speech_power) * rate;
                    let desired = (TARGET_RMS_DB - self.speech_level_db()).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
                    // Fast attack (a loud voice is turned down within
                    // ~100 ms), smooth release (~200 ms on top of the
                    // level average) so the boost never pumps.
                    let alpha = if desired < self.gain_db { 0.10 } else { 0.05 };
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
///   used by Silero VAD, wake preview and the level meters.
/// - `stt_out`: in `auto` mode, a delay-aligned mix of denoised and dry
///   HPF+AGC audio for the recognisers; in `off` and `strong` it stays empty
///   because `vad_out` is used for both.
///
/// Everything is allocated up front (the denoiser too, even when off), so
/// `process` never allocates once its output vectors have grown.
pub struct Conditioner {
    cfg: ConditionerConfig,
    in_resampler: Option<Resampler>,
    hpf: HighPass,
    denoise: Box<DenoiseState<'static>>,
    agc: Agc,
    prev_gain_lin: f32,
    /// Pending 48 kHz post-HPF samples waiting to form a 480-sample frame.
    pending_48k: Vec<f32>,
    /// Previous dry (post-HPF, post-AGC) frame: RNNoise's output lags its
    /// input by exactly one frame, so the dry part of the STT mix is delayed
    /// the same way.
    prev_dry: [f32; RNNOISE_FRAME],
    has_prev_dry: bool,
    decim_vad: Decimator3,
    decim_stt: Decimator3,
    resampled_buf: Vec<f32>,
    /// Telemetry of the last `process` call that completed a frame.
    telemetry: FrameTelemetry,
    /// Wet share of the `auto` STT stream (defaults to `AUTO_STT_WET_MIX`).
    auto_wet_mix: f32,
}

/// Energy sums over the frames of one `process` call.
#[derive(Default)]
struct CallSums {
    frames: u32,
    raw: f32,
    dry: f32,
    post: f32,
}

impl CallSums {
    fn add(&mut self, raw_rms: f32, dry_rms: f32, post_rms: f32) {
        self.frames += 1;
        self.raw += raw_rms * raw_rms;
        self.dry += dry_rms * dry_rms;
        self.post += post_rms * post_rms;
    }

    fn db(sum: f32, frames: u32) -> f32 {
        rms_to_db((sum / frames.max(1) as f32).sqrt())
    }
}

impl Conditioner {
    pub fn new(in_rate: u32, cfg: ConditionerConfig, initial_gain_db: f32) -> Self {
        let start_gain = match cfg.gain {
            GainMode::Manual(db) => db.clamp(MIN_GAIN_DB, MAX_GAIN_DB),
            GainMode::Auto if initial_gain_db.is_finite() => initial_gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB),
            GainMode::Auto => 0.0,
        };
        let in_resampler = (in_rate != WORK_RATE).then(|| Resampler::new(in_rate, WORK_RATE));
        let telemetry = FrameTelemetry { gain_db: start_gain, ..FrameTelemetry::default() };
        Conditioner {
            cfg,
            in_resampler,
            hpf: HighPass::new(WORK_RATE, HIGHPASS_HZ),
            denoise: DenoiseState::new(),
            agc: Agc::new(start_gain),
            prev_gain_lin: db_to_lin(start_gain),
            pending_48k: Vec::with_capacity(RNNOISE_FRAME * 8),
            prev_dry: [0.0; RNNOISE_FRAME],
            has_prev_dry: false,
            decim_vad: Decimator3::new(),
            decim_stt: Decimator3::new(),
            resampled_buf: Vec::with_capacity(4096),
            telemetry,
            auto_wet_mix: AUTO_STT_WET_MIX,
        }
    }

    pub fn config(&self) -> ConditionerConfig {
        self.cfg
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
            self.has_prev_dry = false;
            self.decim_stt.reset();
        }
        if let GainMode::Manual(db) = cfg.gain {
            self.agc.set_gain_db(db);
        }
        self.cfg = cfg;
    }

    /// Update the AGC gain (e.g. after the microphone test calibrated it).
    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.agc.set_gain_db(gain_db);
        self.prev_gain_lin = db_to_lin(self.agc.gain_db());
        self.telemetry.gain_db = self.agc.gain_db();
    }

    /// True when `stt_out` carries a distinct gently-denoised stream.
    pub fn dual_stream(&self) -> bool {
        self.cfg.noise == NoiseSuppression::Auto
    }

    /// Feed mono capture samples at the device rate. Appends 16 kHz
    /// conditioned samples to `vad_out` (and `stt_out` when `noise == Auto`).
    /// Levels in the returned telemetry are averaged over the frames this
    /// call completed (the previous telemetry if it completed none).
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
            for &s in mono_in {
                self.pending_48k.push(self.hpf.tick(s));
            }
        }

        let mut sums = CallSums::default();
        let mut consumed = 0usize;
        while self.pending_48k.len() - consumed >= RNNOISE_FRAME {
            let mut frame = [0.0f32; RNNOISE_FRAME];
            frame.copy_from_slice(&self.pending_48k[consumed..consumed + RNNOISE_FRAME]);
            consumed += RNNOISE_FRAME;
            self.process_frame_48k(&frame, echo_active, vad_out, stt_out, &mut sums);
        }
        if consumed > 0 {
            self.pending_48k.drain(..consumed);
        }
        if sums.frames > 0 {
            self.telemetry.raw_db = CallSums::db(sums.raw, sums.frames);
            self.telemetry.dry_db = CallSums::db(sums.dry, sums.frames);
            self.telemetry.post_db = CallSums::db(sums.post, sums.frames);
        }
        self.telemetry
    }

    fn process_frame_48k(
        &mut self,
        raw_frame: &[f32; RNNOISE_FRAME],
        echo_active: bool,
        vad_out: &mut Vec<f32>,
        stt_out: &mut Vec<f32>,
        sums: &mut CallSums,
    ) {
        let raw_rms = rms_of(raw_frame);
        let raw_db = rms_to_db(raw_rms);
        let target_gain_lin = db_to_lin(self.agc.gain_db());
        let start_gain_lin = self.prev_gain_lin;
        let inv_n = 1.0 / RNNOISE_FRAME as f32;

        // Smoothly ramped gain + soft peak limiter before RNNoise, so quiet
        // mics enter it near -20 dBFS and loud ones never clip it.
        let mut dry = [0.0f32; RNNOISE_FRAME];
        for (i, (d, &x)) in dry.iter_mut().zip(raw_frame.iter()).enumerate() {
            let t = (i + 1) as f32 * inv_n;
            let g = start_gain_lin + (target_gain_lin - start_gain_lin) * t;
            *d = soft_limit(x * g);
        }
        self.prev_gain_lin = target_gain_lin;
        let dry_rms = rms_of(&dry);
        let dry_db = rms_to_db(dry_rms);

        let (post_rms, speech_prob, speech_active) = match self.cfg.noise {
            NoiseSuppression::Off => {
                let active = self.agc.update(self.cfg.gain, raw_db, dry_db, dry_db, None, echo_active);
                self.decim_vad.process(&dry, vad_out);
                (dry_rms, 0.0, active)
            }
            NoiseSuppression::Auto | NoiseSuppression::Strong => {
                // RNNoise works on 16-bit-scaled floats.
                let mut scaled = [0.0f32; RNNOISE_FRAME];
                for (o, &d) in scaled.iter_mut().zip(dry.iter()) {
                    *o = d * 32_768.0;
                }
                let mut out = [0.0f32; RNNOISE_FRAME];
                let prob = self.denoise.process_frame(&mut out, &scaled);
                let mut wet = [0.0f32; RNNOISE_FRAME];
                for (w, &o) in wet.iter_mut().zip(out.iter()) {
                    *w = soft_limit(o / 32_768.0);
                }
                let wet_rms = rms_of(&wet);
                let active = self.agc.update(self.cfg.gain, raw_db, dry_db, rms_to_db(wet_rms), Some(prob), echo_active);
                self.decim_vad.process(&wet, vad_out);

                if self.cfg.noise == NoiseSuppression::Auto {
                    let w = self.auto_wet_mix;
                    let d = 1.0 - w;
                    let mut mixed = wet;
                    if self.has_prev_dry {
                        for (m, (&we, &dr)) in mixed.iter_mut().zip(wet.iter().zip(self.prev_dry.iter())) {
                            *m = soft_limit(w * we + d * dr);
                        }
                    }
                    self.decim_stt.process(&mixed, stt_out);
                }
                self.prev_dry = dry;
                self.has_prev_dry = true;
                (wet_rms, prob, active)
            }
        };
        sums.add(raw_rms, dry_rms, post_rms);
        self.telemetry = FrameTelemetry {
            raw_db,
            dry_db,
            post_db: rms_to_db(post_rms),
            raw_floor_db: self.agc.raw_floor_db(),
            post_floor_db: self.agc.post_floor_db(),
            gain_db: self.agc.gain_db(),
            speech_prob,
            speech_active,
        };
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

/// The gain a session on `device_name` starts with: the manual one, or the
/// gain learned on that microphone last time.
pub fn start_gain_for(cfg: ConditionerConfig, gains: &LearnedGains, device_name: &str) -> f32 {
    match cfg.gain {
        GainMode::Manual(db) => db.clamp(MIN_GAIN_DB, MAX_GAIN_DB),
        GainMode::Auto => lookup_learned_gain(gains, device_name),
    }
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

    /// White noise through a one-pole low-pass (`pole` 0 = white, 0.9 =
    /// hiss-like, 0.98 = fan-like), at `rms_db`.
    fn colored_noise(len: usize, rms_db: f32, pole: f32, seed: u32) -> Vec<f32> {
        let mut lp = 0.0f32;
        let mut out: Vec<f32> = white_noise(len, 0.0, seed).into_iter().map(|w| {
            lp = pole * lp + (1.0 - pole) * w;
            lp
        }).collect();
        let scale = db_to_lin(rms_db) / rms_of(&out).max(1e-9);
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
        let mut within_3db_at = None;
        for (i, chunk) in quiet_speech.chunks(480).enumerate() {
            cond.process(chunk, false, &mut vad_out, &mut stt_out);
            // The ideal gain for -45 dBFS speech is +25 dB.
            if within_3db_at.is_none() && (cond.gain_db() - 25.0).abs() <= 3.0 {
                within_3db_at = Some(i as f32 * 0.01);
            }
        }
        let gain_after_speech = cond.gain_db();
        let converged_s = within_3db_at.expect("never within 3 dB of the target");
        println!("AGC: -45 dBFS speech within 3 dB of -20 dBFS after {converged_s:.2} s, gain {gain_after_speech:.1} dB");
        assert!(converged_s <= 3.0, "too slow: {converged_s} s");
        assert!(
            gain_after_speech > 18.0 && gain_after_speech <= MAX_GAIN_DB,
            "AGC failed to converge on -45 dBFS speech: gain={gain_after_speech:.2} dB"
        );
        // Check last 1 s of conditioned STT audio is near -20 dBFS target
        let tail_db = rms_to_db(rms_of(&stt_out[stt_out.len() - 16_000..]));
        println!("AGC: last second of the recognition stream at {tail_db:.1} dBFS");
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
    fn agc_levels_speech_by_its_vowels_not_its_pauses_and_tails() {
        // Words of 250 ms at -20 dBFS, each with a fading tail and a pause,
        // in a quiet room: a normal microphone needs no boost.
        for (active_db, want) in [(-20.0f32, 0.0f32), (-38.0, 18.0)] {
            let mut cond = Conditioner::new(48_000, ConditionerConfig::default(), 0.0);
            let (mut v, mut st) = (Vec::new(), Vec::new());
            let word = speech_like(48_000, 0.25, active_db);
            let mut signal = Vec::new();
            for _ in 0..16 {
                signal.extend_from_slice(&word);
                let last = word[word.len() - 480..].to_vec();
                for i in 0..12_000 {
                    signal.push(last[i % 480] * (-(i as f32) / 1_900.0).exp());
                }
            }
            let room = white_noise(signal.len(), -75.0, 3);
            for (s, n) in signal.iter_mut().zip(room) {
                *s += n;
            }
            for c in signal.chunks(480) {
                cond.process(c, false, &mut v, &mut st);
            }
            println!("AGC: words at {active_db} dBFS with pauses -> gain {:.1} dB (ideal {want})", cond.gain_db());
            assert!((cond.gain_db() - want).abs() <= 3.0, "gain {} for words at {active_db}", cond.gain_db());
        }
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
        // Mic hiss (pink-ish) and broadband white noise at -45 dBFS, +15 dB
        // gain: -30 dBFS enters RNNoise.
        for (name, pole, settle_s, min_db) in [("hiss", 0.9f32, 1usize, 25.0f32), ("white", 0.0, 2, 15.0)] {
            let mut cond = Conditioner::new(48_000, ConditionerConfig::default(), 15.0);
            let (mut vad_out, mut stt_out) = (Vec::new(), Vec::new());
            let noise = colored_noise(48_000 * (settle_s + 1), -45.0, pole, 555);
            for c in noise.chunks(480) {
                cond.process(c, false, &mut vad_out, &mut stt_out);
            }
            let tail = &vad_out[16_000 * settle_s..];
            let reduced = (-45.0 + 15.0) - rms_to_db(rms_of(tail));
            println!("{name}: suppressed by {reduced:.1} dB after {settle_s} s; stt mix by {:.1} dB", (-30.0) - rms_to_db(rms_of(&stt_out[16_000 * settle_s..])));
            assert!(reduced >= min_db, "{name}: RNNoise only took {reduced:.1} dB off");
            assert!(cond.gain_db() == 15.0, "{name}: AGC moved on noise: {}", cond.gain_db());
        }

        // Echo active: even loud speech-like audio must NOT move AGC gain
        let mut cond = Conditioner::new(48_000, ConditionerConfig::default(), 15.0);
        let (mut vad_out, mut stt_out) = (Vec::new(), Vec::new());
        let before = cond.gain_db();
        let echo_speech = speech_like(48_000, 1.0, -10.0);
        for c in echo_speech.chunks(480) {
            cond.process(c, true, &mut vad_out, &mut stt_out);
        }
        assert_eq!(cond.gain_db(), before, "AGC moved while echo_active=true");
    }

    /// Index of the strongest sample.
    fn peak_at(x: &[f32]) -> usize {
        x.iter().enumerate().fold((0, 0.0f32), |(bi, bv), (i, &v)| if v.abs() > bv { (i, v.abs()) } else { (bi, bv) }).0
    }

    #[test]
    fn latency_stays_within_twenty_ms_and_the_stt_mix_is_aligned() {
        // A click at 0.5 s, fed in odd-sized callbacks like WASAPI does.
        let mut click = vec![0.0f32; 48_000];
        click[24_000] = 0.5;
        // Off: high-pass + decimator only.
        let mut off = Conditioner::new(48_000, ConditionerConfig { gain: GainMode::Manual(0.0), noise: NoiseSuppression::Off }, 0.0);
        let (mut v_off, mut s_off) = (Vec::new(), Vec::new());
        for c in click.chunks(441) {
            off.process(c, false, &mut v_off, &mut s_off);
        }
        let lag_off = peak_at(&v_off) as f32 / 16.0 - 500.0;
        // Auto with a dry-only mix: the dry path is delayed like RNNoise.
        let mut auto = Conditioner::new(48_000, ConditionerConfig { gain: GainMode::Manual(0.0), noise: NoiseSuppression::Auto }, 0.0);
        auto.set_auto_wet_mix(0.0);
        let (mut v_auto, mut s_auto) = (Vec::new(), Vec::new());
        for c in click.chunks(441) {
            auto.process(c, false, &mut v_auto, &mut s_auto);
        }
        let lag_dry = peak_at(&s_auto) as f32 / 16.0 - 500.0;
        println!("latency: off {lag_off:.2} ms, denoised path {lag_dry:.2} ms (plus up to 10 ms of framing)");
        assert!((0.0..=2.0).contains(&lag_off), "off-mode latency {lag_off} ms");
        assert!((lag_dry - lag_off - 10.0).abs() < 0.1, "dry delay {lag_dry} ms is not one RNNoise frame");
        assert!(lag_dry + 10.0 <= 21.0, "worst-case latency {} ms", lag_dry + 10.0);

        // RNNoise's own output lags its input by the same frame: the wet
        // stream lines up with the delayed dry one (best match at lag 0).
        let speech = speech_like(48_000, 2.0, -20.0);
        let mut strong = Conditioner::new(48_000, ConditionerConfig { gain: GainMode::Manual(0.0), noise: NoiseSuppression::Auto }, 0.0);
        strong.set_auto_wet_mix(0.0);
        let (mut wet, mut dry) = (Vec::new(), Vec::new());
        for c in speech.chunks(480) {
            strong.process(c, false, &mut wet, &mut dry);
        }
        let corr = |lag: isize| -> f32 {
            let (a, b) = (&wet[8_000..30_000], &dry[(8_000 + lag) as usize..(30_000 + lag) as usize]);
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            dot / (rms_of(a) * rms_of(b) * a.len() as f32).max(1e-9)
        };
        let best = (-200isize..=200).max_by(|&x, &y| corr(x).total_cmp(&corr(y))).unwrap();
        println!("wet/dry best alignment at lag {best} samples (corr {:.3})", corr(best));
        assert!(best.abs() <= 2, "wet and dry are {best} samples apart");
    }

    #[test]
    fn decimator_rejects_what_would_alias_into_speech() {
        let tone = |hz: f32| -> f32 {
            let mut d = Decimator3::new();
            let input: Vec<f32> = (0..48_000).map(|i| 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / 48_000.0).sin()).collect();
            let mut out = Vec::new();
            d.process(&input, &mut out);
            assert_eq!(out.len(), 16_000);
            rms_to_db(rms_of(&out[1_000..])) - rms_to_db(rms_of(&input))
        };
        let (pass, edge, stop) = (tone(1_000.0), tone(4_000.0), tone(12_000.0));
        println!("decimator: 1 kHz {pass:.2} dB, 4 kHz {edge:.2} dB, 12 kHz {stop:.1} dB");
        assert!(pass.abs() < 0.1);
        assert!(edge > -1.0);
        assert!(stop < -50.0);
    }

    #[test]
    fn peak_normalise_lifts_quiet_utterance_and_ignores_silence() {
        let quiet: Vec<f32> = (0..1600).map(|i| 0.1 * (i as f32 * 0.1).sin()).collect();
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

/// Live models (GigaAM, Silero, a Piper voice) on this PC: speech made by
/// the local voice, 25 dB quieter and mixed with noise, through the old path
/// (plain resampling) and through conditioning. Prints word recall per path.
/// `cargo test --lib live_quiet_noisy_speech -- --ignored --nocapture`
#[cfg(test)]
mod live {
    use super::*;
    use crate::voice::audio::Resampler;
    use crate::voice::stt::Recognizer;
    use crate::voice::vad::{Vad, VadEvent};

    const PHRASES: [&str; 6] = [
        "Джарвис, какая сегодня погода?",
        "Джарвис, поставь таймер на пять минут.",
        "Открой браузер и найди рецепт блинов.",
        "Джарвис, включи музыку погромче.",
        "Напомни мне позвонить маме в семь вечера.",
        "Джарвис, сколько времени?",
    ];

    fn words(t: &str) -> Vec<String> {
        t.to_lowercase()
            .replace('ё', "е")
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn noise(len: usize, rms_db: f32, seed: u32) -> Vec<f32> {
        // Hiss-like: white noise through a one-pole low-pass.
        let (mut x, mut lp) = (seed.max(1), 0.0f32);
        let mut out: Vec<f32> = (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                lp = 0.85 * lp + 0.15 * ((x as f32 / u32::MAX as f32) * 2.0 - 1.0);
                lp
            })
            .collect();
        let k = db_to_lin(rms_db) / rms_of(&out).max(1e-9);
        out.iter_mut().for_each(|s| *s *= k);
        out
    }

    /// `Some(cfg)`: conditioned; `None`: the old path.
    fn run(mic: &[f32], cfg: Option<(ConditionerConfig, f32)>, rec: &Recognizer) -> (Vec<String>, f32) {
        let mut vad = Vad::new(&crate::voice::models::asset("silero-vad").unwrap().path()).unwrap();
        let mut texts = Vec::new();
        let mut cond = cfg.map(|(c, wet)| {
            let mut k = Conditioner::new(48_000, c, 0.0);
            k.set_auto_wet_mix(wet);
            k
        });
        let mut rs = Resampler::new(48_000, 16_000);
        let (mut v, mut s, mut pend_v, mut pend_s) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for block in mic.chunks(480) {
            v.clear();
            s.clear();
            match cond.as_mut() {
                Some(k) => {
                    k.process(block, false, &mut v, &mut s);
                }
                None => rs.process(block, &mut v),
            }
            pend_v.extend_from_slice(&v);
            pend_s.extend_from_slice(if s.len() == v.len() { &s } else { &v });
            while pend_v.len() >= 512 {
                let cv: Vec<f32> = pend_v.drain(..512).collect();
                let cs: Vec<f32> = pend_s.drain(..512).collect();
                for e in vad.push_pair(&cv, &cs) {
                    if let VadEvent::Ended(pcm) = e {
                        let pcm = if cond.is_some() { peak_normalize(&pcm) } else { pcm };
                        texts.push(rec.transcribe(&pcm).map(|t| t.text).unwrap_or_default());
                    }
                }
            }
        }
        (texts, cond.map(|k| k.gain_db()).unwrap_or(0.0))
    }

    fn recall(texts: &[String]) -> (f32, usize) {
        let heard: Vec<String> = texts.iter().flat_map(|t| words(t)).collect();
        let want: Vec<String> = PHRASES.iter().flat_map(|p| words(p)).collect();
        let hit = want.iter().filter(|w| heard.contains(w)).count();
        let names = texts.iter().filter(|t| crate::voice::wake::addressed(t).is_some()).count();
        (hit as f32 / want.len() as f32, names)
    }

    #[test]
    #[ignore]
    fn live_quiet_noisy_speech_is_still_understood() {
        let rec = Recognizer::load_default().expect("GigaAM");
        let names_wanted = PHRASES.iter().filter(|p| p.starts_with("Джарвис")).count();
        let paths: [(&str, Option<(ConditionerConfig, f32)>); 6] = [
            ("old path", None),
            ("off", Some((ConditionerConfig { gain: GainMode::Auto, noise: NoiseSuppression::Off }, 0.0))),
            ("auto wet 0.35", Some((ConditionerConfig::default(), 0.35))),
            ("auto wet 0.50", Some((ConditionerConfig::default(), 0.50))),
            ("auto wet 0.65", Some((ConditionerConfig::default(), 0.65))),
            ("strong", Some((ConditionerConfig { gain: GainMode::Auto, noise: NoiseSuppression::Strong }, 1.0))),
        ];
        // (recall sum, names, runs) per path over the noisy conditions.
        let mut totals = [(0.0f32, 0usize, 0usize); 6];
        for voice_id in ["piper-ruslan", "supertonic-3:1"] {
            let Ok(voice) = crate::voice::neural::NeuralVoice::load(voice_id, 1.0) else {
                println!("{voice_id}: not installed, skipped");
                continue;
            };
            // The «microphone»: 2 s of room, then each phrase and 1.5 s of room.
            let mut clean = vec![0.0f32; 96_000];
            for p in PHRASES {
                let (rate, pcm) = voice.synthesize(p).unwrap();
                let mut up = Resampler::new(rate, 48_000);
                let mut out = Vec::new();
                up.process(&pcm, &mut out);
                let k = db_to_lin(-20.0) / rms_of(&out).max(1e-9);
                clean.extend(out.iter().map(|s| s * k));
                clean.extend(std::iter::repeat(0.0).take(72_000));
            }
            for (label, atten, noise_db) in [("clean -20 dBFS", 0.0f32, -90.0f32), ("-25 dB, room -62", 25.0, -62.0), ("-25 dB, room -55", 25.0, -55.0), ("-25 dB, room -50", 25.0, -50.0)] {
                let room = noise(clean.len(), noise_db, 7);
                let mic: Vec<f32> = clean.iter().zip(&room).map(|(s, n)| s * db_to_lin(-atten) + n).collect();
                for (i, (mode, cfg)) in paths.iter().enumerate() {
                    let (texts, gain) = run(&mic, *cfg, &rec);
                    let (r, n) = recall(&texts);
                    if atten > 0.0 {
                        totals[i].0 += r;
                        totals[i].1 += n;
                        totals[i].2 += 1;
                    }
                    println!("{voice_id:>14} | {label:>16} | {mode:<13} {gain:>5.1} dB | recall {:>5.1}% | names {n}/{names_wanted} | {texts:?}", r * 100.0);
                }
            }
        }
        println!("--- quiet (-25 dB) and noisy, all voices and rooms ---");
        for (i, (mode, _)) in paths.iter().enumerate() {
            let (r, n, runs) = totals[i];
            println!("{mode:<13} | recall {:>5.1}% | names {n}/{}", r / runs.max(1) as f32 * 100.0, runs * names_wanted);
        }
    }
}
