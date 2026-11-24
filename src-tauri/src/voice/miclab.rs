//! Settings → Голос: the live microphone meter and the guided microphone
//! test («Проверить микрофон»).
//!
//! One worker thread listens to the raw microphone (through the running
//! session's stream when it uses the same device, otherwise through its own
//! input-only stream), runs a private `Conditioner` with the settings being
//! edited and publishes the processed level with the noise floor
//! (`voice:mic-meter`). The test records 2 s of silence and 4.5 s of the
//! phrase on the same worker, then recognises the phrase and gives advice.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use super::audio::{self, AudioIo, MicCapture};
use super::conditioner::{self, Conditioner, ConditionerConfig, GainMode, MicTestInput, MicTestResult, NoiseSuppression};
use super::{models, VoiceManager};
use crate::error::{AppError, AppResult};

pub const EVT_MIC_METER: &str = "voice:mic-meter";
pub const EVT_MIC_TEST: &str = "voice:mic-test";

/// The meter keeps running this long after the last request from Settings.
const KEEPALIVE: Duration = Duration::from_secs(5);
const METER_EVERY: Duration = Duration::from_millis(50);
const SILENCE_S: f32 = 2.0;
const SPEECH_S: f32 = 4.5;
/// No audio for this long: the microphone is not delivering.
const NO_AUDIO: Duration = Duration::from_secs(3);

/// The microphone settings being edited (not necessarily saved yet).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicParams {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub gain_mode: String,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub noise: String,
}

impl MicParams {
    pub fn config(&self) -> ConditionerConfig {
        ConditionerConfig {
            gain: GainMode::from_settings(&self.gain_mode, self.gain_db),
            noise: NoiseSuppression::from_settings(&self.noise),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeterEvent {
    /// Processed level (what the detector hears), dBFS.
    db: f32,
    /// Processed noise floor, dBFS.
    floor_db: f32,
    gain_db: f32,
    speech: bool,
    device: String,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestEvent {
    /// silence | speech | analyzing | done | error
    phase: &'static str,
    /// 0..1 within the phase.
    progress: f32,
}

/// What the test recorded, before recognition and advice.
struct Recording {
    device_name: String,
    silence_raw: Vec<f32>,
    silence_post: Vec<f32>,
    speech_raw: Vec<f32>,
    speech_post: Vec<f32>,
    raw_peak: f32,
    /// The phrase at 16 kHz, as the recogniser would get it.
    speech_pcm: Vec<f32>,
    gain_db: f32,
}

enum LabCmd {
    Config(ConditionerConfig),
    Test(tokio::sync::oneshot::Sender<AppResult<Recording>>),
}

struct Worker {
    device: String,
    config: ConditionerConfig,
    ctl: Sender<LabCmd>,
    deadline: Arc<Mutex<Instant>>,
    done: Arc<AtomicBool>,
    /// Why the microphone did not open, once the worker gave up.
    error: Arc<Mutex<Option<String>>>,
}

#[derive(Default)]
pub struct MicLab {
    worker: Mutex<Option<Worker>>,
    testing: AtomicBool,
}

/// Where the worker's raw audio comes from.
enum Source {
    Tap(Arc<AudioIo>),
    Capture(#[allow(dead_code)] MicCapture),
}

impl Drop for Source {
    fn drop(&mut self) {
        if let Source::Tap(a) = self {
            a.set_raw_tap(None);
        }
    }
}

fn open_source(vm: &VoiceManager, device: &str) -> AppResult<(Source, Receiver<(u32, Vec<f32>)>, String)> {
    if let Some(a) = vm.mic_audio() {
        if audio::resolve_input_device_name(device).as_deref() == Some(a.input_device.as_str()) {
            let (tx, rx) = std::sync::mpsc::sync_channel(64);
            a.set_raw_tap(Some(tx));
            let name = a.input_device.clone();
            return Ok((Source::Tap(a), rx, name));
        }
    }
    let (cap, rx) = MicCapture::start(device)?;
    let name = cap.device_name.clone();
    Ok((Source::Capture(cap), rx, name))
}

/// The test's progress through its two phases, counted in samples heard.
struct TestRun {
    reply: tokio::sync::oneshot::Sender<AppResult<Recording>>,
    heard: f32,
    rec: Recording,
    last_event: Instant,
}

impl MicLab {
    /// Keep the meter running (`on`) with these settings, or let it stop.
    pub fn monitor(&self, vm: &Arc<VoiceManager>, on: bool, params: &MicParams) {
        let mut slot = self.worker.lock();
        if !on {
            if let Some(w) = slot.as_ref() {
                *w.deadline.lock() = Instant::now();
            }
            return;
        }
        let cfg = params.config();
        if let Some(w) = slot.as_mut().filter(|w| !w.done.load(Ordering::Acquire) && w.device == params.device) {
            let mut d = w.deadline.lock();
            *d = (*d).max(Instant::now() + KEEPALIVE);
            drop(d);
            if w.config != cfg && w.ctl.send(LabCmd::Config(cfg)).is_ok() {
                w.config = cfg;
            }
            return;
        }
        if let Some(old) = slot.take() {
            *old.deadline.lock() = Instant::now();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let deadline = Arc::new(Mutex::new(Instant::now() + KEEPALIVE));
        let done = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let (me, dl, dn, er, device) = (Arc::clone(vm), deadline.clone(), done.clone(), error.clone(), params.device.clone());
        let spawned = std::thread::Builder::new().name("mic-lab".into()).spawn(move || {
            let mut test = None;
            if let Err(e) = run(&me, &device, cfg, &rx, &dl, &mut test) {
                tracing::warn!("mic meter: {e}");
                meter_error(&me, &device, &e);
                // Before the reply and the queue go away: the test reads it.
                *er.lock() = Some(e.to_string());
                if let Some(t) = test.take() {
                    let _ = t.reply.send(Err(e));
                }
            }
            dn.store(true, Ordering::Release);
            drop(rx);
        });
        if let Err(e) = spawned {
            tracing::warn!("mic meter thread: {e}");
            return;
        }
        *slot = Some(Worker { device: params.device.clone(), config: cfg, ctl: tx, deadline, done, error });
    }

    /// The guided test: silence, then «Джарвис, какая сегодня погода».
    pub async fn test(&self, vm: &Arc<VoiceManager>, params: &MicParams) -> AppResult<MicTestResult> {
        if self.testing.swap(true, Ordering::AcqRel) {
            return Err(AppError::invalid("Проверка микрофона уже идёт"));
        }
        struct Reset<'a>(&'a AtomicBool, &'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
                self.1.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(&self.testing, &vm.mic_test);
        let res = self.record_and_judge(vm, params).await;
        let phase = if res.is_ok() { "done" } else { "error" };
        let _ = vm.app.emit(EVT_MIC_TEST, TestEvent { phase, progress: 1.0 });
        res
    }

    async fn record_and_judge(&self, vm: &Arc<VoiceManager>, params: &MicParams) -> AppResult<MicTestResult> {
        self.monitor(vm, true, params);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let error = {
            let slot = self.worker.lock();
            let w = slot.as_ref().ok_or_else(|| AppError::platform("Не удалось открыть микрофон"))?;
            // Long enough for the whole test even if Settings stops pinging.
            let mut d = w.deadline.lock();
            *d = (*d).max(Instant::now() + Duration::from_secs(20));
            drop(d);
            let _ = w.ctl.send(LabCmd::Test(tx));
            w.error.clone()
        };
        vm.mic_test.store(true, Ordering::Release);
        // The worker gave up (dropping the reply): say why.
        let failed = || AppError::platform(error.lock().clone().unwrap_or_else(|| "Микрофон перестал отвечать".into()));
        let rec = tokio::time::timeout(Duration::from_secs(25), rx)
            .await
            .map_err(|_| AppError::Timeout("Микрофон не прислал звук за время проверки".into()))?
            .map_err(|_| failed())??;
        vm.mic_test.store(false, Ordering::Release);
        let _ = vm.app.emit(EVT_MIC_TEST, TestEvent { phase: "analyzing", progress: 0.0 });

        let asr = models::asset("gigaam-v3").filter(|a| a.installed());
        let stt_available = asr.is_some();
        let transcript = match asr {
            Some(a) => {
                let (engine, path) = (vm.stt.clone(), a.path());
                let pcm = conditioner::peak_normalize(&rec.speech_pcm);
                tokio::task::spawn_blocking(move || engine.transcribe(&path, &pcm))
                    .await
                    .map_err(|e| AppError::internal(e.to_string()))?
                    .map(|t| t.text)
                    .unwrap_or_else(|e| {
                        tracing::warn!("mic test: recognition failed: {e}");
                        String::new()
                    })
            }
            None => String::new(),
        };
        let cfg = params.config();
        let result = conditioner::evaluate_mic_test(&MicTestInput {
            device_name: rec.device_name.clone(),
            silence_raw_dbs: rec.silence_raw,
            silence_post_dbs: rec.silence_post,
            speech_raw_dbs: rec.speech_raw,
            speech_post_dbs: rec.speech_post,
            raw_peak: rec.raw_peak,
            config: cfg,
            agc_gain_db: rec.gain_db,
            transcript,
            stt_available,
        });
        tracing::info!(
            device = %result.device_name,
            noise_floor_db = result.noise_floor_db,
            speech_db = result.speech_level_db,
            snr_db = result.snr_db,
            gain_db = result.applied_gain_db,
            severity = %result.severity,
            words = result.transcript.split_whitespace().count(),
            "mic test"
        );
        // «Авто»: start from the measured gain right away and next time.
        if cfg.gain == GainMode::Auto && result.severity != "error" {
            let mut gains = vm.mic_gains();
            if conditioner::update_learned_gain(&mut gains, &result.device_name, result.applied_gain_db) {
                vm.save_mic_gains(&gains);
            }
            if let Some(a) = vm.mic_audio().filter(|a| a.input_device == result.device_name) {
                a.set_conditioner_gain(result.applied_gain_db);
            }
        }
        Ok(result)
    }
}

fn meter_error(vm: &VoiceManager, device: &str, e: &AppError) {
    let _ = vm.app.emit(
        EVT_MIC_METER,
        MeterEvent { db: -90.0, floor_db: -90.0, gain_db: 0.0, speech: false, device: device.to_string(), error: Some(e.to_string()) },
    );
}

/// The worker: listen, condition, publish levels, record the test.
fn run(
    vm: &Arc<VoiceManager>,
    device: &str,
    mut cfg: ConditionerConfig,
    ctl: &Receiver<LabCmd>,
    deadline: &Mutex<Instant>,
    test: &mut Option<TestRun>,
) -> AppResult<()> {
    let (mut source, mut rx, mut name) = open_source(vm, device)?;
    let gains = vm.mic_gains();
    let mut rate = 0u32;
    let mut cond = Conditioner::new(audio::MIC_RATE, cfg, 0.0);
    let (mut vad_16k, mut stt_16k) = (Vec::with_capacity(4096), Vec::with_capacity(4096));
    let (mut energy, mut samples, mut speech) = (0.0f32, 0usize, false);
    let mut last_meter = Instant::now();
    let mut last_audio = Instant::now();
    let mut reopened = false;
    loop {
        while let Ok(cmd) = ctl.try_recv() {
            match cmd {
                LabCmd::Config(c) => {
                    cfg = c;
                    cond.set_config(c);
                }
                LabCmd::Test(reply) => {
                    let rec = Recording {
                        device_name: name.clone(),
                        silence_raw: Vec::new(),
                        silence_post: Vec::new(),
                        speech_raw: Vec::new(),
                        speech_post: Vec::new(),
                        raw_peak: 0.0,
                        speech_pcm: Vec::with_capacity((SPEECH_S * 16_000.0) as usize + 1024),
                        gain_db: cond.gain_db(),
                    };
                    let _ = vm.app.emit(EVT_MIC_TEST, TestEvent { phase: "silence", progress: 0.0 });
                    *test = Some(TestRun { reply, heard: 0.0, rec, last_event: Instant::now() });
                }
            }
        }
        if test.is_none() && Instant::now() >= *deadline.lock() {
            return Ok(());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok((r, mono)) => {
                last_audio = Instant::now();
                if r != rate {
                    rate = r;
                    cond = Conditioner::new(r, cfg, conditioner::start_gain_for(cfg, &gains, &name));
                }
                vad_16k.clear();
                stt_16k.clear();
                let tel = cond.process(&mono, false, &mut vad_16k, &mut stt_16k);
                let post = conditioner::rms_of(&vad_16k);
                energy += post * post * vad_16k.len() as f32;
                samples += vad_16k.len();
                speech |= tel.speech_active;
                if let Some(t) = test.as_mut() {
                    let in_silence = t.heard < SILENCE_S;
                    t.heard += mono.len() as f32 / r.max(1) as f32;
                    if !vad_16k.is_empty() {
                        let post_db = conditioner::rms_to_db(post);
                        if in_silence {
                            t.rec.silence_raw.push(tel.raw_db);
                            t.rec.silence_post.push(post_db);
                        } else {
                            t.rec.speech_raw.push(tel.raw_db);
                            t.rec.speech_post.push(post_db);
                            let src = if cond.dual_stream() && stt_16k.len() == vad_16k.len() { &stt_16k } else { &vad_16k };
                            t.rec.speech_pcm.extend_from_slice(src);
                            t.rec.raw_peak = mono.iter().fold(t.rec.raw_peak, |m, s| m.max(s.abs()));
                        }
                    }
                    if in_silence && t.heard >= SILENCE_S {
                        let _ = vm.app.emit(EVT_MIC_TEST, TestEvent { phase: "speech", progress: 0.0 });
                        t.last_event = Instant::now();
                    } else if t.last_event.elapsed() >= Duration::from_millis(100) {
                        let (phase, progress) = if t.heard < SILENCE_S {
                            ("silence", t.heard / SILENCE_S)
                        } else {
                            ("speech", (t.heard - SILENCE_S) / SPEECH_S)
                        };
                        let _ = vm.app.emit(EVT_MIC_TEST, TestEvent { phase, progress: progress.min(1.0) });
                        t.last_event = Instant::now();
                    }
                    if t.heard >= SILENCE_S + SPEECH_S {
                        let mut t = test.take().expect("test");
                        t.rec.gain_db = cond.gain_db();
                        let _ = t.reply.send(Ok(t.rec));
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if last_audio.elapsed() >= NO_AUDIO {
                    if let Some(t) = test.take() {
                        let _ = t.reply.send(Err(AppError::platform("Микрофон не присылает звук. Проверьте, что он подключён и не выключен в Windows.")));
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The session closed or reopened its microphone: listen again
                // (once in a row), through whatever is open now.
                drop(source);
                if reopened {
                    return Err(AppError::platform("Микрофон перестал отвечать"));
                }
                reopened = true;
                (source, rx, name) = open_source(vm, device)?;
                rate = 0;
                continue;
            }
        }
        if samples > 0 {
            reopened = false;
        }
        if last_meter.elapsed() >= METER_EVERY && samples > 0 {
            let tel = cond.telemetry();
            let db = conditioner::rms_to_db((energy / samples as f32).sqrt());
            let _ = vm.app.emit(
                EVT_MIC_METER,
                MeterEvent { db, floor_db: tel.post_floor_db, gain_db: tel.gain_db, speech, device: name.clone(), error: None },
            );
            (energy, samples, speech) = (0.0, 0, false);
            last_meter = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_params_map_to_conditioner_config() {
        let p: MicParams = serde_json::from_str(r#"{"device":"","gainMode":"manual","gainDb":40,"noise":"strong"}"#).unwrap();
        assert_eq!(p.config(), ConditionerConfig { gain: GainMode::Manual(conditioner::MAX_GAIN_DB), noise: NoiseSuppression::Strong });
        let p: MicParams = serde_json::from_str("{}").unwrap();
        assert_eq!(p.config(), ConditionerConfig::default());
    }
}
