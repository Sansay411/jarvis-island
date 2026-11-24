//! Native audio I/O for voice sessions (cpal → WASAPI).
//!
//! Streams live on a dedicated thread (cpal streams are not Send). The mic
//! path downmixes to mono, low-pass/resamples to 16 kHz PCM16 and hands
//! 32 ms chunks to the session; the speaker path plays 24 kHz PCM16 from the
//! model through a queue that can be cleared instantly for barge-in. Real RMS
//! levels of both directions are published for the island's visualiser.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::mpsc;

use super::conditioner::{self, start_gain_for, Conditioner, ConditionerConfig, FrameTelemetry, LearnedGains};
use crate::error::{AppError, AppResult};

pub const MIC_RATE: u32 = 16_000;
/// The input name the debug WAV feeder reports.
const TEST_INPUT_NAME: &str = "тестовые WAV-файлы";

/// Do not pile up stuck threads for a wedged endpoint, but allow selecting
/// another microphone/output while the old Windows call has not returned.
#[derive(Default)]
struct OpeningDevices(Mutex<HashSet<(String, String)>>);
impl OpeningDevices {
    fn acquire(&self, input: &str, output: &str) -> Option<OpeningGuard<'_>> {
        let key = (input.to_owned(), output.to_owned());
        self.0.lock().insert(key.clone()).then(|| OpeningGuard { devices: self, key })
    }
}
struct OpeningGuard<'a> { devices: &'a OpeningDevices, key: (String, String) }
impl Drop for OpeningGuard<'_> {
    fn drop(&mut self) { self.devices.0.lock().remove(&self.key); }
}
static OPENING: std::sync::LazyLock<OpeningDevices> = std::sync::LazyLock::new(OpeningDevices::default);
pub const SPEAKER_RATE: u32 = 24_000;
const CHUNK: usize = 512; // Silero window: 32 ms at 16 kHz
// About 1 s: a short stall of the session loop must not drop the start of
// «Джарвис…»; older audio is still discarded (never seconds of old commands).
const MIC_QUEUE: usize = 32;
const MAX_MIC_DELAY: std::time::Duration = std::time::Duration::from_millis(1000);

#[derive(Default)]
pub struct Levels {
    input: AtomicU32,
    output: AtomicU32,
}

impl Levels {
    fn set(slot: &AtomicU32, v: f32) {
        slot.store(v.to_bits(), Ordering::Relaxed);
    }
    pub fn input(&self) -> f32 {
        f32::from_bits(self.input.load(Ordering::Relaxed))
    }
    pub fn output(&self) -> f32 {
        f32::from_bits(self.output.load(Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevices {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_input: Option<String>,
    pub default_output: Option<String>,
}

fn dev_name(d: &cpal::Device) -> String {
    d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| "?".into())
}

pub fn list_devices() -> AudioDevices {
    let host = cpal::default_host();
    let names = |it: Result<_, _>| -> Vec<String> {
        match it {
            Ok(devs) => {
                let devs: Vec<cpal::Device> = devs;
                devs.iter().map(dev_name).collect()
            }
            Err(_) => Vec::new(),
        }
    };
    AudioDevices {
        inputs: names(host.input_devices().map(|d| d.collect::<Vec<_>>())),
        outputs: names(host.output_devices().map(|d| d.collect::<Vec<_>>())),
        default_input: host.default_input_device().map(|d| dev_name(&d)),
        default_output: host.default_output_device().map(|d| dev_name(&d)),
    }
}

/// Streaming linear resampler with a short box low-pass for downsampling.
pub(crate) struct Resampler {
    ratio: f64, // input samples per output sample
    pos: f64,
    prev: f32,
    taps: Vec<f32>,
    tap_i: usize,
}

impl Resampler {
    pub(crate) fn new(from: u32, to: u32) -> Self {
        let ratio = from as f64 / to as f64;
        let n = if ratio > 1.0 { ratio.ceil() as usize } else { 1 };
        Resampler { ratio, pos: 0.0, prev: 0.0, taps: vec![0.0; n.max(1)], tap_i: 0 }
    }

    pub(crate) fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &raw in input {
            // Box filter (moving average) before decimation.
            self.taps[self.tap_i] = raw;
            self.tap_i = (self.tap_i + 1) % self.taps.len();
            let x = self.taps.iter().sum::<f32>() / self.taps.len() as f32;
            while self.pos <= 1.0 {
                out.push(self.prev + (x - self.prev) * self.pos as f32);
                self.pos += self.ratio;
            }
            self.pos -= 1.0;
            self.prev = x;
        }
    }
}

/// Map RMS to a perceptual 0..1 level for the visualiser.
fn perceptual(r: f32) -> f32 {
    if r <= 1e-5 {
        return 0.0;
    }
    let db = 20.0 * r.log10(); // -100..0
    ((db + 55.0) / 50.0).clamp(0.0, 1.0)
}

/// Non-blocking raw mono audio tap `(sample_rate, mono_samples)` used by
/// Settings' live level meter and guided mic test when a session is open.
pub type RawTapTx = std::sync::mpsc::SyncSender<(u32, Vec<f32>)>;

struct ConditionerControl {
    cfg: Mutex<ConditionerConfig>,
    override_gain: Mutex<Option<f32>>,
    gen: AtomicU64,
    has_tap: AtomicBool,
    raw_tap: Mutex<Option<RawTapTx>>,
}

impl ConditionerControl {
    fn new(cfg: ConditionerConfig) -> Self {
        ConditionerControl {
            cfg: Mutex::new(cfg),
            override_gain: Mutex::new(None),
            gen: AtomicU64::new(0),
            has_tap: AtomicBool::new(false),
            raw_tap: Mutex::new(None),
        }
    }

    fn set_config(&self, cfg: ConditionerConfig) {
        *self.cfg.lock() = cfg;
        self.gen.fetch_add(1, Ordering::Release);
    }

    fn set_gain_db(&self, gain_db: f32) {
        *self.override_gain.lock() = Some(gain_db);
        self.gen.fetch_add(1, Ordering::Release);
    }

    fn set_raw_tap(&self, tap: Option<RawTapTx>) {
        let on = tap.is_some();
        *self.raw_tap.lock() = tap;
        self.has_tap.store(on, Ordering::Release);
    }

    fn sync(&self, cond: &mut Conditioner, seen_gen: &mut u64) {
        let g = self.gen.load(Ordering::Acquire);
        if g != *seen_gen {
            *seen_gen = g;
            let cfg = *self.cfg.lock();
            cond.set_config(cfg);
            if let Some(gain) = self.override_gain.lock().take() {
                cond.set_gain_db(gain);
            }
        }
    }

    fn push_tap(&self, rate: u32, mono: &[f32]) {
        if !self.has_tap.load(Ordering::Acquire) || mono.is_empty() {
            return;
        }
        let mut guard = self.raw_tap.lock();
        if let Some(tx) = guard.as_ref() {
            if let Err(std::sync::mpsc::TrySendError::Disconnected(_)) = tx.try_send((rate, mono.to_vec())) {
                *guard = None;
                self.has_tap.store(false, Ordering::Release);
            }
        }
    }
}

enum Cmd {
    Stop,
}

pub struct AudioIo {
    cmd: std::sync::mpsc::Sender<Cmd>,
    playback: Arc<Mutex<VecDeque<f32>>>,
    out_rate: u32,
    pub in_rate: u32,
    pub start_gain_db: f32,
    pub levels: Arc<Levels>,
    play_resampler: Mutex<Resampler>,
    pub input_device: String,
    pub output_device: String,
    /// Output goes to headphones: JARVIS cannot hear itself, so no echo
    /// gating — the user may interrupt at normal loudness.
    pub headphones: bool,
    /// Held: the speaker plays silence and keeps the queue, so a reply can
    /// stop the instant the user starts talking and go on if it was only
    /// the room (barge-in).
    paused: Arc<AtomicBool>,
    cond_ctl: Arc<ConditionerControl>,
}

/// Resolve a configured input device name (or empty string for the OS default)
/// to the actual endpoint name Windows/macOS reports.
pub fn resolve_input_device_name(name: &str) -> Option<String> {
    let host = cpal::default_host();
    find_device(&host, name, true).map(|d| dev_name(&d))
}

/// Headphones and headsets by the endpoint name Windows shows.
pub fn is_headphones(name: &str) -> bool {
    let n = name.to_lowercase();
    ["наушник", "headphone", "headset", "гарнитур", "earphone", "airpods", "buds", "головной телефон", "hands-free"].iter().any(|k| n.contains(k))
}

/// One 32 ms (512-sample) 16 kHz conditioned microphone chunk.
#[derive(Debug, Clone, PartialEq)]
pub struct MicChunk {
    /// Conditioned audio for VAD, wake preview and level decisions.
    pub vad: Vec<f32>,
    /// The gently denoised mix for recognition in `auto` noise mode
    /// (`None`: the same as `vad`).
    pub stt: Option<Vec<f32>>,
    /// Pre-gain (post high-pass) level of this stretch in dBFS.
    pub raw_db: f32,
    /// Gain applied, in dB (AGC or manual).
    pub gain_db: f32,
    /// Pre-gain room floor in dBFS.
    pub raw_floor_db: f32,
    /// The AGC has heard enough speech for its gain to be worth saving.
    pub learned: bool,
}

impl MicChunk {
    pub fn stt_samples(&self) -> &[f32] {
        self.stt.as_deref().unwrap_or(&self.vad)
    }

    /// The recognition stream as PCM16 (realtime engine).
    pub fn stt_pcm16(&self) -> Vec<i16> {
        self.stt_samples().iter().map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect()
    }
}

/// Cuts conditioned 16 kHz audio into 512-sample `MicChunk`s, keeping the
/// detection and recognition streams aligned sample for sample.
#[derive(Default)]
struct Chunker {
    vad: Vec<f32>,
    stt: Vec<f32>,
    raw_energy: f32,
    raw_samples: usize,
}

impl Chunker {
    /// `gated`: the echo gate closed for this stretch (silence instead).
    fn push(&mut self, cond: &Conditioner, tel: &FrameTelemetry, vad_16k: &[f32], stt_16k: &[f32], gated: bool, mut emit: impl FnMut(MicChunk)) {
        let dual = cond.dual_stream();
        if dual && self.stt.len() != self.vad.len() {
            self.stt.clear();
            self.stt.extend_from_slice(&self.vad);
        } else if !dual {
            self.stt.clear();
        }
        if gated {
            self.vad.resize(self.vad.len() + vad_16k.len(), 0.0);
            if dual {
                self.stt.resize(self.stt.len() + vad_16k.len(), 0.0);
            }
        } else {
            self.vad.extend_from_slice(vad_16k);
            if dual {
                self.stt.extend_from_slice(stt_16k);
            }
        }
        let raw = conditioner::db_to_lin(tel.raw_db);
        self.raw_energy += raw * raw * vad_16k.len() as f32;
        self.raw_samples += vad_16k.len();
        while self.vad.len() >= CHUNK {
            let raw_db = conditioner::rms_to_db((self.raw_energy / self.raw_samples.max(1) as f32).sqrt());
            self.raw_energy = 0.0;
            self.raw_samples = 0;
            let stt = (dual && self.stt.len() >= CHUNK).then(|| self.stt.drain(..CHUNK).collect());
            emit(MicChunk {
                vad: self.vad.drain(..CHUNK).collect(),
                stt,
                raw_db,
                gain_db: tel.gain_db,
                raw_floor_db: tel.raw_floor_db,
                learned: cond.has_learned_gain(),
            });
        }
    }
}

pub struct MicStream {
    rx: mpsc::Receiver<CapturedChunk>,
    pub stale_chunks: u64,
}

struct CapturedChunk {
    chunk: MicChunk,
    captured_at: std::time::Instant,
}

impl MicStream {
    pub async fn recv(&mut self) -> Option<MicChunk> {
        while let Some(item) = self.rx.recv().await {
            if item.captured_at.elapsed() <= MAX_MIC_DELAY {
                return Some(item.chunk);
            }
            self.stale_chunks += 1;
        }
        None
    }
}

fn find_device(host: &cpal::Host, name: &str, input: bool) -> Option<cpal::Device> {
    if name.trim().is_empty() {
        return if input { host.default_input_device() } else { host.default_output_device() };
    }
    let devs = if input { host.input_devices().ok()?.collect::<Vec<_>>() } else { host.output_devices().ok()?.collect::<Vec<_>>() };
    devs.into_iter()
        .find(|d| dev_name(d) == name)
        .or_else(|| if input { host.default_input_device() } else { host.default_output_device() })
}

/// Open the microphone and capture for a moment, nothing else. Run in a
/// fresh process (`jarvis.exe --probe-mic NAME`) it tells a microphone that
/// is really broken from an audio stack wedged inside the running app: after
/// one IAudioClient::Initialize hangs, every later attempt in the same
/// process fails («Параметр задан неверно», 2026-11-18) while a new process
/// opens the device at once.
pub fn probe_input(name: &str) -> Result<String, String> {
    let host = cpal::default_host();
    let dev = find_device(&host, name, true).ok_or("Микрофон не найден")?;
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let heard = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = heard.clone();
    let err = |e| tracing::warn!("probe stream error: {e}");
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => dev.build_input_stream::<f32, _, _>(cfg.config(), move |d, _| { h.fetch_add(d.len(), Ordering::Relaxed); }, err, None),
        SampleFormat::I16 => dev.build_input_stream::<i16, _, _>(cfg.config(), move |d, _| { h.fetch_add(d.len(), Ordering::Relaxed); }, err, None),
        SampleFormat::U16 => dev.build_input_stream::<u16, _, _>(cfg.config(), move |d, _| { h.fetch_add(d.len(), Ordering::Relaxed); }, err, None),
        other => return Err(format!("формат {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(400));
    drop(stream);
    match heard.load(Ordering::Relaxed) {
        0 => Err("микрофон открылся, но не прислал звук".into()),
        n => Ok(format!("{}: {n} samples", dev_name(&dev))),
    }
}

impl AudioIo {
    pub fn start(
        input_name: &str,
        output_name: &str,
        cond_cfg: ConditionerConfig,
        learned_gains: LearnedGains,
    ) -> AppResult<(Arc<AudioIo>, MicStream)> {
        let opening = OPENING.acquire(input_name, output_name).ok_or_else(|| AppError::Timeout(
            "Это аудиоустройство всё ещё не ответило на прошлую попытку подключения. Переподключите его или выберите другой микрофон в настройках голоса.".into(),
        ))?;
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
        let (mic_tx, mic_rx) = mpsc::channel::<CapturedChunk>(MIC_QUEUE);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<AppResult<(u32, u32, f32, String, String)>>();
        let playback = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(SPEAKER_RATE as usize * 4)));
        let levels = Arc::new(Levels::default());
        let speaking = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let cond_ctl = Arc::new(ConditionerControl::new(cond_cfg));

        // Debug builds only: JARVIS_VOICE_INPUT_WAV=a.wav;b.wav replaces the
        // microphone with 16 kHz WAV files played in real time (end-to-end
        // tests of the voice pipeline without a person speaking).
        let test_input: Option<Vec<std::path::PathBuf>> = if cfg!(debug_assertions) {
            std::env::var("JARVIS_VOICE_INPUT_WAV").ok().map(|v| v.split(';').map(std::path::PathBuf::from).collect())
        } else {
            None
        };
        if let Some(files) = test_input.clone() {
            spawn_test_feeder(files, mic_tx.clone(), levels.clone(), cond_cfg, start_gain_for(cond_cfg, &learned_gains, TEST_INPUT_NAME));
        }
        let test_mode = test_input.is_some();
        // With test input, JARVIS_VOICE_OUTPUT=null also replaces the speaker:
        // queued audio is consumed in real time without any device, so the
        // pipeline can be tested when the machine's audio stack is unusable.
        let null_output = test_mode && std::env::var("JARVIS_VOICE_OUTPUT").is_ok_and(|v| v == "null");
        // What the audio thread is doing, to name the device if it hangs
        // (a wedged USB microphone blocks IAudioClient::Initialize).
        let stage = Arc::new(Mutex::new(String::from("Список аудиоустройств")));
        let stage_t = stage.clone();
        let (pb, lv, sp, pz, ctl_t) = (playback.clone(), levels.clone(), speaking.clone(), paused.clone(), cond_ctl.clone());
        let (in_name, out_name) = (input_name.to_string(), output_name.to_string());
        std::thread::Builder::new()
            .name("voice-audio".into())
            .spawn(move || {
                if null_output {
                    drop(opening);
                    let start_gain = start_gain_for(cond_cfg, &learned_gains, TEST_INPUT_NAME);
                    let _ = ready_tx.send(Ok((MIC_RATE, SPEAKER_RATE, start_gain, TEST_INPUT_NAME.into(), "без устройства (тест)".into())));
                    let frames = SPEAKER_RATE as usize / 50; // 20 ms
                    while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = cmd_rx.recv_timeout(std::time::Duration::from_millis(20)) {
                        if pz.load(Ordering::Acquire) {
                            Levels::set(&lv.output, 0.0);
                            sp.store(false, Ordering::Relaxed);
                            continue;
                        }
                        let mut q = pb.lock();
                        let n = frames.min(q.len());
                        let sq: f32 = q.drain(..n).map(|s| s * s).sum();
                        let playing = !q.is_empty();
                        drop(q);
                        let level = (sq / frames as f32).sqrt();
                        Levels::set(&lv.output, perceptual(level));
                        sp.store(playing || level > 0.001, Ordering::Relaxed);
                    }
                    return;
                }
                let host = cpal::default_host();
                let opened = std::time::Instant::now();
                let result = (|| -> AppResult<(Option<cpal::Stream>, cpal::Stream, u32, u32, f32, String, String)> {
                    let input = find_device(&host, &in_name, true).ok_or_else(|| AppError::not_found("Микрофон не найден"))?;
                    let output = find_device(&host, &out_name, false).ok_or_else(|| AppError::not_found("Устройство вывода звука не найдено"))?;
                    let in_dev_name = dev_name(&input);
                    let out_dev_name = dev_name(&output);
                    *stage_t.lock() = format!("Микрофон «{in_dev_name}»");
                    let in_cfg = input.default_input_config().map_err(|e| AppError::platform(format!("Микрофон: {e}")))?;
                    *stage_t.lock() = format!("Вывод звука «{out_dev_name}»");
                    let out_cfg = output.default_output_config().map_err(|e| AppError::platform(format!("Вывод звука: {e}")))?;

                    // ---- microphone ----
                    let in_conf: StreamConfig = in_cfg.config();
                    let in_ch = in_conf.channels as usize;
                    let in_rate = in_conf.sample_rate;
                    let start_gain = start_gain_for(cond_cfg, &learned_gains, &in_dev_name);
                    let mut cond = Conditioner::new(in_rate, cond_cfg, start_gain);
                    let mut seen_gen = ctl_t.gen.load(Ordering::Acquire);
                    let mut mono = Vec::with_capacity(4096);
                    let mut vad_16k = Vec::with_capacity(2048);
                    let mut stt_16k = Vec::with_capacity(2048);
                    let mut chunker = Chunker::default();
                    let lv_in = lv.clone();
                    let sp_in = sp.clone();
                    let ctl_in = ctl_t.clone();
                    let phones_in = is_headphones(&out_dev_name);
                    let mut on_input = move |frame_iter: &mut dyn Iterator<Item = f32>| {
                        mono.clear();
                        let mut acc = 0.0;
                        let mut n = 0;
                        for s in frame_iter {
                            acc += s;
                            n += 1;
                            if n == in_ch {
                                mono.push(acc / in_ch as f32);
                                acc = 0.0;
                                n = 0;
                            }
                        }
                        if mono.is_empty() {
                            return;
                        }
                        ctl_in.push_tap(in_rate, &mono);
                        ctl_in.sync(&mut cond, &mut seen_gen);

                        let echo_active = !phones_in && sp_in.load(Ordering::Relaxed);
                        vad_16k.clear();
                        stt_16k.clear();
                        let tel = cond.process(&mono, echo_active, &mut vad_16k, &mut stt_16k);
                        Levels::set(&lv_in.input, perceptual(conditioner::db_to_lin(tel.post_db)));
                        // Echo gate: while JARVIS speaks through speakers, quiet
                        // pickup is replaced by silence so the detector does not
                        // interrupt on our own voice; real barge-in is louder.
                        // Compared after the gain (frozen while JARVIS speaks),
                        // so a quiet microphone is judged like a normal one.
                        let out_level = f32::from_bits(lv_in.output.load(Ordering::Relaxed));
                        let gated = echo_active && perceptual(conditioner::db_to_lin(tel.dry_db)) < out_level * 0.85;
                        chunker.push(&cond, &tel, &vad_16k, &stt_16k, gated, |chunk| {
                            let _ = mic_tx.try_send(CapturedChunk { chunk, captured_at: std::time::Instant::now() });
                        });
                    };
                    let err = |e| tracing::warn!("mic stream error: {e}");
                    *stage_t.lock() = format!("Микрофон «{in_dev_name}»");
                    // In test mode the WAV feeder replaces the microphone entirely.
                    let in_stream = if test_mode { None } else { Some(match in_cfg.sample_format() {
                        SampleFormat::F32 => input.build_input_stream::<f32, _, _>(in_conf, move |d, _| on_input(&mut d.iter().copied()), err, None),
                        SampleFormat::I16 => input.build_input_stream::<i16, _, _>(in_conf, move |d, _| on_input(&mut d.iter().map(|s| *s as f32 / 32768.0)), err, None),
                        SampleFormat::U16 => input.build_input_stream::<u16, _, _>(in_conf, move |d, _| on_input(&mut d.iter().map(|s| (*s as f32 - 32768.0) / 32768.0)), err, None),
                        other => return Err(AppError::unsupported(format!("Формат микрофона {other:?} не поддерживается"))),
                    }
                    .map_err(|e| AppError::platform(format!("Не удалось открыть микрофон: {e}")))?) };

                    // ---- speaker ----
                    *stage_t.lock() = format!("Вывод звука «{out_dev_name}»");
                    let out_conf: StreamConfig = out_cfg.config();
                    let out_ch = out_conf.channels as usize;
                    let out_rate = out_conf.sample_rate;
                    let (pb_out, lv_out, sp_out, pz_out) = (pb.clone(), lv.clone(), sp.clone(), pz.clone());
                    let fill = move |buf: &mut dyn FnMut(usize, f32), frames: usize| {
                        if pz_out.load(Ordering::Acquire) {
                            // Held: silence, nothing consumed; the echo gate is
                            // open so the user's words reach the recogniser whole.
                            for i in 0..frames * out_ch {
                                buf(i, 0.0);
                            }
                            Levels::set(&lv_out.output, 0.0);
                            sp_out.store(false, Ordering::Relaxed);
                            return;
                        }
                        let mut q = pb_out.lock();
                        let mut sq = 0.0f32;
                        for f in 0..frames {
                            let s = q.pop_front().unwrap_or(0.0);
                            sq += s * s;
                            for c in 0..out_ch {
                                buf(f * out_ch + c, s);
                            }
                        }
                        let playing = !q.is_empty();
                        drop(q);
                        let level = if frames > 0 { (sq / frames as f32).sqrt() } else { 0.0 };
                        Levels::set(&lv_out.output, perceptual(level));
                        sp_out.store(playing || level > 0.001, Ordering::Relaxed);
                    };
                    let err_out = |e| tracing::warn!("speaker stream error: {e}");
                    let out_stream = match out_cfg.sample_format() {
                        SampleFormat::F32 => output.build_output_stream::<f32, _, _>(
                            out_conf,
                            move |d: &mut [f32], _| {
                                let frames = d.len() / out_ch;
                                fill(&mut |i, s| d[i] = s, frames)
                            },
                            err_out,
                            None,
                        ),
                        SampleFormat::I16 => output.build_output_stream::<i16, _, _>(
                            out_conf,
                            move |d: &mut [i16], _| {
                                let frames = d.len() / out_ch;
                                fill(&mut |i, s| d[i] = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16, frames)
                            },
                            err_out,
                            None,
                        ),
                        other => return Err(AppError::unsupported(format!("Формат вывода {other:?} не поддерживается"))),
                    }
                    .map_err(|e| AppError::platform(format!("Не удалось открыть вывод звука: {e}")))?;

                    if let Some(s) = &in_stream {
                        s.play().map_err(|e| AppError::platform(format!("Микрофон: {e}")))?;
                    }
                    out_stream.play().map_err(|e| AppError::platform(format!("Вывод звука: {e}")))?;
                    Ok((in_stream, out_stream, in_rate, out_rate, start_gain, in_dev_name, out_dev_name))
                })();
                // Devices answered (opened or failed): another attempt may start.
                drop(opening);
                match result {
                    Ok((in_stream, out_stream, in_rate, out_rate, start_gain, in_dev, out_dev)) => {
                        // Logged even when the caller already gave up waiting:
                        // shows how slow a device really was.
                        tracing::info!(
                            ms = opened.elapsed().as_millis() as u64,
                            in_rate,
                            start_gain_db = format_args!("{start_gain:.1}"),
                            gain_mode = cond_cfg.gain.label(),
                            noise = cond_cfg.noise.label(),
                            input = %in_dev,
                            output = %out_dev,
                            "audio devices opened"
                        );
                        let _ = ready_tx.send(Ok((in_rate, out_rate, start_gain, in_dev, out_dev)));
                        // Keep streams alive until told to stop.
                        let _ = cmd_rx.recv();
                        drop(in_stream);
                        drop(out_stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }

            })
            .map_err(|e| AppError::internal(format!("audio thread: {e}")))?;

        let (in_rate, out_rate, start_gain_db, input_device, output_device) = ready_rx
            // The fifine USB microphone has taken 5–21 s to open (after
            // reconnecting, after idle); the island shows «Запускаю» meanwhile.
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| AppError::Timeout(format!("{} не отвечает — переподключите устройство или выберите другое в Настройках → Голос", stage.lock())))??;
        Ok((
            Arc::new(AudioIo {
                cmd: cmd_tx,
                playback,
                out_rate,
                in_rate,
                start_gain_db,
                levels,
                play_resampler: Mutex::new(Resampler::new(SPEAKER_RATE, out_rate)),
                headphones: is_headphones(&output_device),
                input_device,
                output_device,
                paused,
                cond_ctl,
            }),
            MicStream { rx: mic_rx, stale_chunks: 0 },
        ))
    }

    /// Live-update microphone conditioning settings on the running stream.
    pub fn set_conditioner_config(&self, cfg: ConditionerConfig) {
        self.cond_ctl.set_config(cfg);
    }

    /// Override the conditioner's current AGC gain (e.g. after mic calibration).
    pub fn set_conditioner_gain(&self, gain_db: f32) {
        self.cond_ctl.set_gain_db(gain_db);
    }

    /// Attach or detach a non-blocking raw mono tap for Settings' live meter
    /// or guided microphone test.
    pub fn set_raw_tap(&self, tap: Option<RawTapTx>) {
        self.cond_ctl.set_raw_tap(tap);
    }

    /// Queue model audio (PCM16 mono at 24 kHz).
    pub fn play_pcm16(&self, pcm: &[i16]) {
        let input: Vec<f32> = pcm.iter().map(|s| *s as f32 / 32768.0).collect();
        let mut out = Vec::with_capacity(input.len() * self.out_rate as usize / SPEAKER_RATE as usize + 8);
        self.play_resampler.lock().process(&input, &mut out);
        let mut q = self.playback.lock();
        q.extend(out);
        // Hard cap (~90 s) so a runaway stream can't grow memory unbounded.
        let cap = self.out_rate as usize * 90;
        while q.len() > cap {
            q.pop_front();
        }
    }

    /// A soft two-note cue (~150 ms) played the moment JARVIS hears its
    /// name, before the command is finished and recognised.
    /// Heard the name: a short rising cue.
    pub fn chime() -> Vec<i16> {
        Self::tones(&[(880.0, 60.0), (1318.5, 90.0)], 0.16)
    }

    /// Back to waiting for the name: the same cue, falling.
    pub fn chime_down() -> Vec<i16> {
        Self::tones(&[(1318.5, 60.0), (880.0, 90.0)], 0.12)
    }

    /// An action waits for «Разрешить» on the island: the same note twice.
    pub fn chime_confirm() -> Vec<i16> {
        Self::tones(&[(1046.5, 70.0), (0.0, 60.0), (1046.5, 70.0)], 0.13)
    }

    /// Something did not work: two low notes.
    pub fn chime_error() -> Vec<i16> {
        Self::tones(&[(392.0, 90.0), (311.1, 140.0)], 0.14)
    }

    /// Sine notes (Hz, ms) one after another at `gain` of full scale.
    fn tones(notes: &[(f32, f32)], gain: f32) -> Vec<i16> {
        let rate = SPEAKER_RATE as f32;
        let mut out = Vec::new();
        for &(freq, ms) in notes {
            let n = (rate * ms / 1000.0) as usize;
            for i in 0..n {
                let t = i as f32 / rate;
                // Raised-cosine envelope: no clicks at either end.
                let env = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
                out.push((gain * env * (2.0 * std::f32::consts::PI * freq * t).sin() * i16::MAX as f32) as i16);
            }
        }
        out
    }

    /// Barge-in: drop everything not yet played.
    pub fn clear_playback(&self) {
        self.playback.lock().clear();
        self.paused.store(false, Ordering::Release);
    }

    /// Hold or release the speaker without dropping what is queued.
    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Ordering::Release);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub fn is_playing(&self) -> bool {
        !self.playback.lock().is_empty()
    }

    pub fn stop(&self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

/// See `JARVIS_VOICE_INPUT_WAV` in `AudioIo::start`. 2 s of silence, then
/// each file followed by `JARVIS_VOICE_INPUT_GAP_S` (default 25) seconds of
/// silence so the agent can answer, then silence until the session ends.
/// `JARVIS_VOICE_INPUT_ATTEN_DB` (e.g. 25) plays the files that much quieter,
/// like a weak microphone. The files go through the same conditioning.
fn spawn_test_feeder(
    files: Vec<std::path::PathBuf>,
    tx: mpsc::Sender<CapturedChunk>,
    levels: Arc<Levels>,
    cond_cfg: ConditionerConfig,
    start_gain: f32,
) {
    let chunks_per_second = MIC_RATE as usize / CHUNK;
    let gap: usize = std::env::var("JARVIS_VOICE_INPUT_GAP_S").ok().and_then(|v| v.parse().ok()).unwrap_or(25) * chunks_per_second;
    let atten: f32 = std::env::var("JARVIS_VOICE_INPUT_ATTEN_DB").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let scale = conditioner::db_to_lin(-atten.abs()) / 32768.0;
    tracing::warn!(?files, atten_db = atten.abs(), "voice test input: microphone replaced by WAV files");
    let _ = std::thread::Builder::new().name("voice-test-input".into()).spawn(move || {
        let mut cond = Conditioner::new(MIC_RATE, cond_cfg, start_gain);
        let mut vad_16k = Vec::with_capacity(CHUNK * 2);
        let mut stt_16k = Vec::with_capacity(CHUNK * 2);
        let mut chunker = Chunker::default();
        let mut send = |raw_chunk: &[i16]| -> bool {
            let f: Vec<f32> = raw_chunk.iter().map(|s| *s as f32 * scale).collect();
            vad_16k.clear();
            stt_16k.clear();
            let tel = cond.process(&f, false, &mut vad_16k, &mut stt_16k);
            Levels::set(&levels.input, perceptual(conditioner::db_to_lin(tel.post_db)));
            std::thread::sleep(std::time::Duration::from_secs_f64(CHUNK as f64 / MIC_RATE as f64));
            let mut alive = true;
            chunker.push(&cond, &tel, &vad_16k, &stt_16k, false, |chunk| {
                alive &= tx.blocking_send(CapturedChunk { chunk, captured_at: std::time::Instant::now() }).is_ok();
            });
            alive
        };
        let silence = vec![0i16; CHUNK];
        for _ in 0..chunks_per_second * 2 {
            if !send(&silence) {
                return;
            }
        }
        for file in files {
            let Ok(bytes) = std::fs::read(&file) else { continue };
            let pcm: Vec<i16> = bytes.get(44..).unwrap_or_default().chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            for c in pcm.chunks(CHUNK) {
                let mut c = c.to_vec();
                c.resize(CHUNK, 0);
                if !send(&c) {
                    return;
                }
            }
            for _ in 0..gap {
                if !send(&silence) {
                    return;
                }
            }
        }
        while send(&silence) {}
    });
}

/// Lightweight input-only microphone stream for Settings' live level meter and
/// guided microphone test when no voice session is active (or when previewing
/// a different microphone). Never opens the speaker.
pub struct MicCapture {
    cmd: std::sync::mpsc::Sender<Cmd>,
    pub device_name: String,
    pub sample_rate: u32,
}

impl MicCapture {
    pub fn start(input_name: &str) -> AppResult<(MicCapture, std::sync::mpsc::Receiver<(u32, Vec<f32>)>)> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
        let (data_tx, data_rx) = std::sync::mpsc::sync_channel::<(u32, Vec<f32>)>(64);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<AppResult<(String, u32)>>();
        let in_name = input_name.to_string();

        std::thread::Builder::new()
            .name("mic-capture".into())
            .spawn(move || {
                let host = cpal::default_host();
                let res = (|| -> AppResult<(cpal::Stream, String, u32)> {
                    let input = find_device(&host, &in_name, true)
                        .ok_or_else(|| AppError::not_found("Микрофон не найден"))?;
                    let name = dev_name(&input);
                    let in_cfg = input
                        .default_input_config()
                        .map_err(|e| AppError::platform(format!("Микрофон: {e}")))?;
                    let conf: StreamConfig = in_cfg.config();
                    let ch = (conf.channels as usize).max(1);
                    let rate = conf.sample_rate;
                    let mut mono = Vec::with_capacity(2048);
                    let mut on_in = move |it: &mut dyn Iterator<Item = f32>| {
                        mono.clear();
                        let mut acc = 0.0;
                        let mut n = 0;
                        for s in it {
                            acc += s;
                            n += 1;
                            if n == ch {
                                mono.push(acc / ch as f32);
                                acc = 0.0;
                                n = 0;
                            }
                        }
                        if !mono.is_empty() {
                            let _ = data_tx.try_send((rate, mono.clone()));
                        }
                    };
                    let err = |e| tracing::warn!("mic capture stream error: {e}");
                    let stream = match in_cfg.sample_format() {
                        SampleFormat::F32 => input.build_input_stream::<f32, _, _>(conf, move |d, _| on_in(&mut d.iter().copied()), err, None),
                        SampleFormat::I16 => input.build_input_stream::<i16, _, _>(conf, move |d, _| on_in(&mut d.iter().map(|s| *s as f32 / 32768.0)), err, None),
                        SampleFormat::U16 => input.build_input_stream::<u16, _, _>(conf, move |d, _| on_in(&mut d.iter().map(|s| (*s as f32 - 32768.0) / 32768.0)), err, None),
                        other => return Err(AppError::unsupported(format!("Формат микрофона {other:?} не поддерживается"))),
                    }
                    .map_err(|e| AppError::platform(format!("Не удалось открыть микрофон: {e}")))?;
                    stream.play().map_err(|e| AppError::platform(format!("Микрофон: {e}")))?;
                    Ok((stream, name, rate))
                })();
                match res {
                    Ok((stream, name, rate)) => {
                        let _ = ready_tx.send(Ok((name, rate)));
                        let _ = cmd_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| AppError::internal(format!("mic capture thread: {e}")))?;

        let (device_name, sample_rate) = ready_rx
            .recv_timeout(std::time::Duration::from_secs(12))
            .map_err(|_| AppError::Timeout("Микрофон не отвечает — переподключите устройство или выберите другое".into()))??;
        Ok((MicCapture { cmd: cmd_tx, device_name, sample_rate }, data_rx))
    }

    pub fn stop(&self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

impl Drop for AudioIo {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

/// Lightweight output-only WASAPI audio stream (`cpal`) for universal TTS
/// (`tts_speak`, `speak_aloud`, chat readouts, reminders, focus completion).
/// Never opens the microphone, so Windows never shows a mic indicator and PCs
/// without a microphone play crystal-clear 24 kHz neural speech in ~10 ms.
pub struct AudioOutput {
    cmd: std::sync::mpsc::Sender<Cmd>,
    playback: Arc<Mutex<VecDeque<f32>>>,
    out_rate: u32,
    pub levels: Arc<Levels>,
    play_resampler: Mutex<Resampler>,
    pub output_device: String,
    speaking: Arc<AtomicBool>,
}

impl AudioOutput {
    pub fn start(output_name: &str) -> AppResult<Arc<AudioOutput>> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<AppResult<(u32, String)>>();
        let playback = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(SPEAKER_RATE as usize * 4)));
        let levels = Arc::new(Levels::default());
        let speaking = Arc::new(AtomicBool::new(false));

        let null_output = cfg!(debug_assertions) && std::env::var("JARVIS_VOICE_OUTPUT").is_ok_and(|v| v == "null");
        let (pb, lv, sp) = (playback.clone(), levels.clone(), speaking.clone());
        let out_name = output_name.to_string();

        std::thread::Builder::new()
            .name("tts-audio-out".into())
            .spawn(move || {
                if null_output {
                    let _ = ready_tx.send(Ok((SPEAKER_RATE, "без устройства (тест)".into())));
                    let frames = SPEAKER_RATE as usize / 50;
                    while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                        cmd_rx.recv_timeout(std::time::Duration::from_millis(20))
                    {
                        let mut q = pb.lock();
                        let n = frames.min(q.len());
                        let sq: f32 = q.drain(..n).map(|s| s * s).sum();
                        let playing = !q.is_empty();
                        drop(q);
                        let level = (sq / frames as f32).sqrt();
                        Levels::set(&lv.output, perceptual(level));
                        sp.store(playing || level > 0.001, Ordering::Relaxed);
                    }
                    return;
                }

                let host = cpal::default_host();
                let res = (|| -> AppResult<(cpal::Stream, u32, String)> {
                    let output = find_device(&host, &out_name, false)
                        .ok_or_else(|| AppError::not_found("Устройство вывода звука не найдено"))?;
                    let out_cfg = output
                        .default_output_config()
                        .map_err(|e| AppError::platform(format!("Вывод звука: {e}")))?;
                    let out_conf: StreamConfig = out_cfg.config();
                    let out_ch = out_conf.channels as usize;
                    let out_rate = out_conf.sample_rate;
                    let (pb_out, lv_out, sp_out) = (pb.clone(), lv.clone(), sp.clone());

                    let fill = move |buf: &mut dyn FnMut(usize, f32), frames: usize| {
                        let mut q = pb_out.lock();
                        let mut sq = 0.0f32;
                        for f in 0..frames {
                            let s = q.pop_front().unwrap_or(0.0);
                            sq += s * s;
                            for c in 0..out_ch {
                                buf(f * out_ch + c, s);
                            }
                        }
                        let playing = !q.is_empty();
                        drop(q);
                        let level = if frames > 0 { (sq / frames as f32).sqrt() } else { 0.0 };
                        Levels::set(&lv_out.output, perceptual(level));
                        sp_out.store(playing || level > 0.001, Ordering::Relaxed);
                    };

                    let err_out = |e| tracing::warn!("tts speaker stream error: {e}");
                    let out_stream = match out_cfg.sample_format() {
                        SampleFormat::F32 => output.build_output_stream::<f32, _, _>(
                            out_conf,
                            move |d: &mut [f32], _| {
                                let frames = d.len() / out_ch.max(1);
                                fill(&mut |i, s| d[i] = s, frames)
                            },
                            err_out,
                            None,
                        ),
                        SampleFormat::I16 => output.build_output_stream::<i16, _, _>(
                            out_conf,
                            move |d: &mut [i16], _| {
                                let frames = d.len() / out_ch.max(1);
                                fill(&mut |i, s| d[i] = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16, frames)
                            },
                            err_out,
                            None,
                        ),
                        other => return Err(AppError::unsupported(format!("Формат вывода {other:?} не поддерживается"))),
                    }
                    .map_err(|e| AppError::platform(format!("Не удалось открыть вывод звука: {e}")))?;

                    out_stream.play().map_err(|e| AppError::platform(format!("Вывод звука: {e}")))?;
                    Ok((out_stream, out_rate, dev_name(&output)))
                })();

                match res {
                    Ok((stream, out_rate, out_dev)) => {
                        let _ = ready_tx.send(Ok((out_rate, out_dev)));
                        let _ = cmd_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| AppError::internal(format!("tts audio thread: {e}")))?;

        let (out_rate, output_device) = ready_rx
            .recv_timeout(std::time::Duration::from_secs(8))
            .map_err(|_| AppError::Timeout("Устройство вывода звука не отвечает".into()))??;

        Ok(Arc::new(AudioOutput {
            cmd: cmd_tx,
            playback,
            out_rate,
            levels,
            play_resampler: Mutex::new(Resampler::new(SPEAKER_RATE, out_rate)),
            output_device,
            speaking,
        }))
    }

    pub fn play_pcm16(&self, pcm: &[i16]) {
        if pcm.is_empty() {
            return;
        }
        let input: Vec<f32> = pcm.iter().map(|s| *s as f32 / 32768.0).collect();
        let mut out = Vec::with_capacity(input.len() * self.out_rate as usize / SPEAKER_RATE as usize + 8);
        self.play_resampler.lock().process(&input, &mut out);
        let mut q = self.playback.lock();
        q.extend(out);
        let cap = self.out_rate as usize * 120;
        while q.len() > cap {
            q.pop_front();
        }
        self.speaking.store(true, Ordering::Relaxed);
    }

    pub fn clear_playback(&self) {
        self.playback.lock().clear();
        Levels::set(&self.levels.output, 0.0);
        self.speaking.store(false, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        !self.playback.lock().is_empty() || self.speaking.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_stalled_device_blocks_duplicate_attempts_but_not_switching_devices() {
        let devices = super::OpeningDevices::default();
        let first = devices.acquire("USB", "speakers").unwrap();
        assert!(devices.acquire("USB", "speakers").is_none());
        assert!(devices.acquire("USB", "speakers").is_none());
        let other = devices.acquire("built-in", "speakers").unwrap();
        drop(other);
        assert!(devices.acquire("USB", "speakers").is_none());
        drop(first);
        assert!(devices.acquire("USB", "speakers").is_some());
    }
    use super::*;

    #[tokio::test]
    async fn delayed_audio_is_discarded_instead_of_replayed_seconds_later() {
        let (tx, rx) = mpsc::channel(8);
        let mk = |v: f32| MicChunk { vad: vec![v; 512], stt: None, raw_db: -40.0, gain_db: 20.0, raw_floor_db: -60.0, learned: false };
        tx.send(CapturedChunk { chunk: mk(0.1), captured_at: std::time::Instant::now() - std::time::Duration::from_secs(2) }).await.unwrap();
        tx.send(CapturedChunk { chunk: mk(0.2), captured_at: std::time::Instant::now() }).await.unwrap();
        drop(tx);
        let mut mic = MicStream { rx, stale_chunks: 0 };
        assert_eq!(mic.recv().await.unwrap().vad, vec![0.2; 512]);
        assert_eq!(mic.stale_chunks, 1);
        assert!(mic.recv().await.is_none());
    }

    #[test]
    fn resampler_ratios() {
        let mut r = Resampler::new(48_000, 16_000);
        let input = vec![0.5f32; 4800];
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert!((out.len() as i64 - 1600).abs() <= 2, "{}", out.len());
        let mut up = Resampler::new(24_000, 48_000);
        let mut out2 = Vec::new();
        up.process(&vec![0.1f32; 2400], &mut out2);
        assert!((out2.len() as i64 - 4800).abs() <= 2, "{}", out2.len());
    }

    /// Live check (outside the sandbox): open every microphone with its
    /// default format and with each supported one, report what Windows says.
    #[test]
    #[ignore]
    fn live_open_mic() {
        let host = cpal::default_host();
        let def = host.default_input_device().map(|d| dev_name(&d));
        println!("default input: {def:?}");
        println!("probe: {:?}", probe_input(""));
        for dev in host.input_devices().unwrap() {
            let name = dev_name(&dev);
            match dev.default_input_config() {
                Ok(cfg) => {
                    let r = match cfg.sample_format() {
                        SampleFormat::I16 => dev.build_input_stream::<i16, _, _>(cfg.config(), |_d: &[i16], _| {}, |e| println!("  stream error: {e}"), None),
                        _ => dev.build_input_stream::<f32, _, _>(cfg.config(), |_d: &[f32], _| {}, |e| println!("  stream error: {e}"), None),
                    };
                    println!("{name}: default {cfg:?} -> {}", r.as_ref().map(|_| "OK".to_string()).unwrap_or_else(|e| e.to_string()));
                }
                Err(e) => println!("{name}: default config error {e}"),
            }
            if let Ok(all) = dev.supported_input_configs() {
                for c in all {
                    println!("  supports {c:?}");
                }
            }
        }
    }

    #[test]
    fn perceptual_levels() {
        assert_eq!(perceptual(0.0), 0.0);
        assert!(perceptual(0.3) > 0.7);
        assert!(perceptual(0.003) < 0.2);
    }
}
