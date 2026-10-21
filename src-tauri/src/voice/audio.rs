//! Native audio I/O for voice sessions (cpal → WASAPI).
//!
//! Streams live on a dedicated thread (cpal streams are not Send). The mic
//! path downmixes to mono, low-pass/resamples to 16 kHz PCM16 and hands
//! 100 ms chunks to the session; the speaker path plays 24 kHz PCM16 from the
//! model through a queue that can be cleared instantly for barge-in. Real RMS
//! levels of both directions are published for the island's visualiser.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};

pub const MIC_RATE: u32 = 16_000;
pub const SPEAKER_RATE: u32 = 24_000;
const CHUNK: usize = 1600; // 100 ms at 16 kHz

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
struct Resampler {
    ratio: f64, // input samples per output sample
    pos: f64,
    prev: f32,
    taps: Vec<f32>,
    tap_i: usize,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        let ratio = from as f64 / to as f64;
        let n = if ratio > 1.0 { ratio.ceil() as usize } else { 1 };
        Resampler { ratio, pos: 0.0, prev: 0.0, taps: vec![0.0; n.max(1)], tap_i: 0 }
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
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

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Map RMS to a perceptual 0..1 level for the visualiser.
fn perceptual(r: f32) -> f32 {
    if r <= 1e-5 {
        return 0.0;
    }
    let db = 20.0 * r.log10(); // -100..0
    ((db + 55.0) / 50.0).clamp(0.0, 1.0)
}

enum Cmd {
    Stop,
}

pub struct AudioIo {
    cmd: std::sync::mpsc::Sender<Cmd>,
    playback: Arc<Mutex<VecDeque<f32>>>,
    out_rate: u32,
    pub levels: Arc<Levels>,
    play_resampler: Mutex<Resampler>,
    pub input_device: String,
    pub output_device: String,
}

pub struct MicStream {
    pub rx: mpsc::Receiver<Vec<i16>>,
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

impl AudioIo {
    pub fn start(input_name: &str, output_name: &str) -> AppResult<(Arc<AudioIo>, MicStream)> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
        let (mic_tx, mic_rx) = mpsc::channel::<Vec<i16>>(64);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<AppResult<(u32, String, String)>>();
        let playback = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(SPEAKER_RATE as usize * 4)));
        let levels = Arc::new(Levels::default());
        let speaking = Arc::new(AtomicBool::new(false));

        let (pb, lv, sp) = (playback.clone(), levels.clone(), speaking.clone());
        let (in_name, out_name) = (input_name.to_string(), output_name.to_string());
        std::thread::Builder::new()
            .name("voice-audio".into())
            .spawn(move || {
                let host = cpal::default_host();
                let result = (|| -> AppResult<(cpal::Stream, cpal::Stream, u32, String, String)> {
                    let input = find_device(&host, &in_name, true).ok_or_else(|| AppError::not_found("Микрофон не найден"))?;
                    let output = find_device(&host, &out_name, false).ok_or_else(|| AppError::not_found("Устройство вывода звука не найдено"))?;
                    let in_cfg = input.default_input_config().map_err(|e| AppError::platform(format!("Микрофон: {e}")))?;
                    let out_cfg = output.default_output_config().map_err(|e| AppError::platform(format!("Вывод звука: {e}")))?;

                    // ---- microphone ----
                    let in_conf: StreamConfig = in_cfg.config();
                    let in_ch = in_conf.channels as usize;
                    let mut rs = Resampler::new(in_conf.sample_rate, MIC_RATE);
                    let mut mono = Vec::with_capacity(4096);
                    let mut resampled = Vec::with_capacity(2048);
                    let mut pending: Vec<i16> = Vec::with_capacity(CHUNK * 2);
                    let lv_in = lv.clone();
                    let sp_in = sp.clone();
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
                        let level = rms(&mono);
                        Levels::set(&lv_in.input, perceptual(level));
                        // Echo gate: while JARVIS speaks through speakers, quiet
                        // pickup is replaced by silence so the server VAD does not
                        // interrupt on our own voice; real barge-in is louder.
                        let out_level = f32::from_bits(lv_in.output.load(Ordering::Relaxed));
                        let gated = sp_in.load(Ordering::Relaxed) && perceptual(level) < out_level * 0.85;
                        resampled.clear();
                        rs.process(&mono, &mut resampled);
                        for &s in &resampled {
                            let v = if gated { 0.0 } else { s };
                            pending.push((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                        }
                        while pending.len() >= CHUNK {
                            let chunk: Vec<i16> = pending.drain(..CHUNK).collect();
                            let _ = mic_tx.try_send(chunk);
                        }
                    };
                    let err = |e| tracing::warn!("mic stream error: {e}");
                    let in_stream = match in_cfg.sample_format() {
                        SampleFormat::F32 => input.build_input_stream::<f32, _, _>(in_conf, move |d, _| on_input(&mut d.iter().copied()), err, None),
                        SampleFormat::I16 => input.build_input_stream::<i16, _, _>(in_conf, move |d, _| on_input(&mut d.iter().map(|s| *s as f32 / 32768.0)), err, None),
                        SampleFormat::U16 => input.build_input_stream::<u16, _, _>(in_conf, move |d, _| on_input(&mut d.iter().map(|s| (*s as f32 - 32768.0) / 32768.0)), err, None),
                        other => return Err(AppError::unsupported(format!("Формат микрофона {other:?} не поддерживается"))),
                    }
                    .map_err(|e| AppError::platform(format!("Не удалось открыть микрофон: {e}")))?;

                    // ---- speaker ----
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

                    in_stream.play().map_err(|e| AppError::platform(format!("Микрофон: {e}")))?;
                    out_stream.play().map_err(|e| AppError::platform(format!("Вывод звука: {e}")))?;
                    Ok((in_stream, out_stream, out_rate, dev_name(&input), dev_name(&output)))
                })();
                match result {
                    Ok((in_stream, out_stream, out_rate, in_dev, out_dev)) => {
                        let _ = ready_tx.send(Ok((out_rate, in_dev, out_dev)));
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

        let (out_rate, input_device, output_device) = ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| AppError::Timeout("Аудиоустройства не ответили".into()))??;
        Ok((
            Arc::new(AudioIo {
                cmd: cmd_tx,
                playback,
                out_rate,
                levels,
                play_resampler: Mutex::new(Resampler::new(SPEAKER_RATE, out_rate)),
                input_device,
                output_device,
            }),
            MicStream { rx: mic_rx },
        ))
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

    /// Barge-in: drop everything not yet played.
    pub fn clear_playback(&self) {
        self.playback.lock().clear();
    }

    pub fn is_playing(&self) -> bool {
        !self.playback.lock().is_empty()
    }

    pub fn stop(&self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

impl Drop for AudioIo {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn perceptual_levels() {
        assert_eq!(perceptual(0.0), 0.0);
        assert!(perceptual(0.3) > 0.7);
        assert!(perceptual(0.003) < 0.2);
    }
}
