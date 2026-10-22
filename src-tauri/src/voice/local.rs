//! Local voice pipeline: microphone → Silero VAD → GigaAM (sherpa-onnx) →
//! the text agent (same model, tools, confirmations and chat history as typed
//! requests) → neural speech synthesis → speakers.
//!
//! Speech starts with the first complete sentence of the reply. Speaking
//! over JARVIS clears queued speech at once (barge-in). A new request while a
//! voice turn is still running waits for it (the latest one wins); only
//! «стоп»/«хватит» cancel — live use showed that cancelling on every remark
//! («спасибо») killed long tasks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::audio::{AudioIo, SPEAKER_RATE};
use super::phrases::PhraseSplitter;
use super::models;
use super::stt::{self, Recognizer, Transcript};
use super::tts::Speaker;
use super::vad::{Vad, VadEvent};
use super::{LevelEvent, TranscriptEvent, VoiceManager, VoiceState, EVT_LEVEL, EVT_TRANSCRIPT};
use crate::agent::Agent;
use crate::error::{AppError, AppResult};
use crate::store::settings::VoiceSettings;

/// The loaded recognizer, kept between sessions and freed when idle.
#[derive(Default)]
pub struct SttEngine {
    loaded: Mutex<Option<Recognizer>>,
    last_used: Mutex<Option<Instant>>,
}

/// Free the model after this long without a voice session.
const UNLOAD_AFTER: Duration = Duration::from_secs(180);

impl SttEngine {
    fn ensure(&self, dir: &Path) -> AppResult<()> {
        let mut g = self.loaded.lock();
        if g.as_ref().map(|r| r.dir != dir).unwrap_or(true) {
            *g = None; // free the old model before loading another
            *g = Some(Recognizer::load(dir)?);
        }
        Ok(())
    }

    fn transcribe(&self, path: &Path, pcm: &[f32]) -> AppResult<Transcript> {
        self.ensure(path)?;
        *self.last_used.lock() = Some(Instant::now());
        let g = self.loaded.lock();
        g.as_ref().ok_or_else(|| AppError::internal("model not loaded"))?.transcribe(pcm)
    }

    pub fn unload_if_idle(&self) {
        let idle = self.last_used.lock().map(|t| t.elapsed() >= UNLOAD_AFTER).unwrap_or(true);
        if idle && self.loaded.lock().take().is_some() {
            tracing::info!("speech recogniser unloaded after idle");
        }
    }
}

const STOP_WORDS: &[&str] = &["стоп", "хватит", "отмена", "отмени", "замолчи", "тихо", "стой"];

fn is_stop(text: &str) -> bool {
    let t: String = text.to_lowercase().chars().filter(|c| c.is_alphabetic() || c.is_whitespace()).collect();
    let words: Vec<&str> = t.split_whitespace().filter(|w| *w != "джарвис" && *w != "джарвиз").collect();
    !words.is_empty() && words.len() <= 2 && words.iter().all(|w| STOP_WORDS.contains(w))
}

enum Ev {
    Transcribed(AppResult<Transcript>, Instant),
    Delta(u64, String),
    TurnDone(u64, AppResult<String>),
    Audio(u64, AppResult<Vec<i16>>),
}

struct Pipeline {
    vm: Arc<VoiceManager>,
    audio: Arc<AudioIo>,
    ev_tx: mpsc::UnboundedSender<Ev>,
    synth_tx: mpsc::UnboundedSender<(u64, String)>,
    model_path: PathBuf,
    transcribing: usize,
    turn_seq: u64,
    turn_active: bool,
    pending_request: Option<String>,
    splitter: PhraseSplitter,
    reply: String,
    spoken_any: bool,
    speech_gen: u64,
    queued_phrases: usize,
    speech_end_at: Option<Instant>,
    first_audio_pending: bool,
    shown: VoiceState,
}

impl Pipeline {
    fn emit_transcript(&self, role: &'static str, text: &str, is_final: bool) {
        let _ = self.vm.app.emit(EVT_TRANSCRIPT, TranscriptEvent { role, text: text.to_string(), is_final });
    }

    fn say(&mut self, phrase: String) {
        self.queued_phrases += 1;
        self.spoken_any = true;
        let _ = self.synth_tx.send((self.speech_gen, phrase));
    }

    /// Drop everything queued or playing.
    fn hush(&mut self) {
        self.audio.clear_playback();
        self.speech_gen += 1;
        self.queued_phrases = 0;
    }

    fn start_turn(&mut self, text: String) {
        self.turn_seq += 1;
        self.turn_active = true;
        self.splitter = PhraseSplitter::default();
        self.reply.clear();
        self.spoken_any = false;
        let seq = self.turn_seq;
        let (dtx, mut drx) = mpsc::unbounded_channel::<String>();
        let fwd = self.ev_tx.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(d) = drx.recv().await {
                let _ = fwd.send(Ev::Delta(seq, d));
            }
        });
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        let done = self.ev_tx.clone();
        tauri::async_runtime::spawn(async move {
            let res = agent.voice_turn(text, dtx).await;
            let _ = done.send(Ev::TurnDone(seq, res));
        });
    }

    fn on_request(&mut self, text: String) {
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        if is_stop(&text) {
            tracing::info!(busy = agent.is_busy(), "stop word");
            self.hush();
            if agent.is_busy() {
                agent.cancel();
            }
            self.pending_request = None;
            return;
        }
        if self.turn_active {
            // Runs after the current turn; a newer request replaces it.
            tracing::info!("request queued behind the running voice turn");
            self.pending_request = Some(text);
        } else if agent.is_busy() {
            self.say("Сейчас я выполняю задачу из чата. Скажите ещё раз, когда я закончу.".into());
        } else {
            self.start_turn(text);
        }
    }

    fn derive_state(&self, vad: &Vad) -> VoiceState {
        if vad.is_active() {
            VoiceState::Hearing
        } else if self.audio.is_playing() {
            VoiceState::Speaking
        } else if self.transcribing > 0 || self.turn_active || self.queued_phrases > 0 {
            VoiceState::Thinking
        } else {
            VoiceState::Listening
        }
    }

    fn refresh_state(&mut self, vad: &Vad) {
        let s = self.derive_state(vad);
        if s != self.shown {
            self.shown = s;
            self.vm.set_state(s, None, None);
        }
    }
}

impl VoiceManager {
    pub(super) async fn run_local(self: &Arc<Self>, cfg: VoiceSettings, cancel: CancellationToken) -> AppResult<()> {
        let (asr, silero) = (models::asset("gigaam-v3").expect("catalog"), models::asset("silero-vad").expect("catalog"));
        if !asr.installed() || !silero.installed() {
            return Err(AppError::invalid("Модель распознавания речи не скачана — откройте Настройки → Голос"));
        }
        let model_path = asr.path();
        let mut vad = Vad::new(&silero.path())?;
        let store = &self.app.state::<crate::state::AppState>().store;
        crate::ai::client_from_settings(&store.settings(), store)?;

        // Load the model while the microphone already listens.
        let engine = self.stt.clone();
        let p = model_path.clone();
        let preload = tokio::task::spawn_blocking(move || engine.ensure(&p));

        let (voice, rate) = (cfg.tts_voice.clone(), cfg.tts_rate as f64);
        let speaker = tokio::task::spawn_blocking(move || {
            Speaker::new(&voice, rate).or_else(|e| {
                // A missing or broken neural voice must not silence JARVIS.
                tracing::warn!("voice {voice:?} unavailable, using Windows voice: {e}");
                Speaker::new(super::tts::WINDOWS_PREFIX, rate)
            })
        })
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
        let speaker = Arc::new(speaker);
        let (audio, mut mic) = AudioIo::start(&cfg.input_device, &cfg.output_device)?;
        self.set_state(VoiceState::Listening, None, Some((&audio.input_device, &audio.output_device)));
        tracing::info!(voice = %speaker.name(), "local voice session started");

        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<Ev>();
        // Phrases are synthesised one at a time, in order.
        let (synth_tx, mut synth_rx) = mpsc::unbounded_channel::<(u64, String)>();
        {
            let ev_tx = ev_tx.clone();
            let speaker = speaker.clone();
            tauri::async_runtime::spawn(async move {
                while let Some((gen, phrase)) = synth_rx.recv().await {
                    let sp = speaker.clone();
                    let pcm = tokio::task::spawn_blocking(move || sp.synthesize(&phrase, SPEAKER_RATE))
                        .await
                        .unwrap_or_else(|e| Err(AppError::internal(e.to_string())));
                    let _ = ev_tx.send(Ev::Audio(gen, pcm));
                }
            });
        }

        let mut pl = Pipeline {
            vm: Arc::clone(self),
            audio: audio.clone(),
            ev_tx: ev_tx.clone(),
            synth_tx,
            model_path,
            transcribing: 0,
            turn_seq: 0,
            turn_active: false,
            pending_request: None,
            splitter: PhraseSplitter::default(),
            reply: String::new(),
            spoken_any: false,
            speech_gen: 0,
            queued_phrases: 0,
            speech_end_at: None,
            first_audio_pending: false,
            shown: VoiceState::Listening,
        };
        let mut level_tick = tokio::time::interval(Duration::from_millis(33));

        let result: AppResult<()> = loop {
            tokio::select! {
                _ = cancel.cancelled() => break Err(AppError::Cancelled),
                chunk = mic.rx.recv() => {
                    let Some(chunk) = chunk else { break Err(AppError::platform("Микрофон перестал отвечать")) };
                    let pcm: Vec<f32> = chunk.iter().map(|s| *s as f32 / 32768.0).collect();
                    // Our own voice through speakers must be louder to count.
                    vad.echo_guard = pl.audio.is_playing();
                    for event in vad.push(&pcm) { match event {
                        VadEvent::Started => {
                            if pl.audio.is_playing() || pl.queued_phrases > 0 {
                                let t = Instant::now();
                                pl.hush();
                                let ms = t.elapsed().as_millis() as u64;
                                self.metrics.lock().last_barge_in_ms = Some(ms);
                                tracing::info!(ms, "barge-in: speech cleared");
                            }
                        }
                        VadEvent::Ended(utterance) => {
                            pl.transcribing += 1;
                            let ended = Instant::now();
                            let engine = self.stt.clone();
                            let path = pl.model_path.clone();
                            let tx = ev_tx.clone();
                            tokio::task::spawn_blocking(move || {
                                let res = engine.transcribe(&path, &utterance);
                                let _ = tx.send(Ev::Transcribed(res, ended));
                            });
                        }
                    } }
                    pl.refresh_state(&vad);
                }
                Some(ev) = ev_rx.recv() => match ev {
                    Ev::Transcribed(res, ended) => {
                        pl.transcribing = pl.transcribing.saturating_sub(1);
                        match res {
                            Ok(t) => {
                                self.metrics.lock().last_stt_ms = Some(t.ms);
                                let words = t.text.split_whitespace().count();
                                tracing::info!(ms = t.ms, words, "speech recognised");
                                if !stt::is_meaningful(&t.text) {
                                    tracing::debug!("ignored non-speech: {:?}", t.text);
                                } else {
                                    pl.emit_transcript("user", &t.text, true);
                                    pl.speech_end_at = Some(ended);
                                    pl.first_audio_pending = true;
                                    pl.on_request(t.text);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("speech recognition failed: {e}");
                                break Err(e);
                            }
                        }
                        pl.refresh_state(&vad);
                    }
                    Ev::Delta(seq, d) if seq == pl.turn_seq => {
                        pl.reply.push_str(&d);
                        pl.emit_transcript("assistant", &d, false);
                        for phrase in pl.splitter.push(&d) {
                            pl.say(phrase);
                        }
                    }
                    Ev::Delta(..) => {}
                    Ev::TurnDone(seq, res) if seq == pl.turn_seq => {
                        pl.turn_active = false;
                        match res {
                            Ok(final_text) => {
                                if let Some(rest) = pl.splitter.finish() {
                                    pl.say(rest);
                                } else if !pl.spoken_any && !final_text.trim().is_empty() {
                                    pl.say(super::phrases::speakable(&final_text));
                                }
                                let shown = if pl.reply.trim().is_empty() { final_text } else { pl.reply.clone() };
                                pl.emit_transcript("assistant", shown.trim(), true);
                            }
                            Err(e) => {
                                let msg = e.to_string();
                                if pl.pending_request.is_none() && !msg.contains("отмен") && !msg.to_lowercase().contains("cancel") {
                                    pl.say(format!("Не получилось: {}", super::phrases::speakable(&msg)));
                                }
                            }
                        }
                        if let Some(next) = pl.pending_request.take() {
                            pl.hush();
                            pl.start_turn(next);
                        }
                        pl.refresh_state(&vad);
                    }
                    Ev::TurnDone(..) => {}
                    Ev::Audio(gen, res) if gen == pl.speech_gen => {
                        pl.queued_phrases = pl.queued_phrases.saturating_sub(1);
                        match res {
                            Ok(pcm) => {
                                pl.audio.play_pcm16(&pcm);
                                if pl.first_audio_pending {
                                    pl.first_audio_pending = false;
                                    if let Some(t0) = pl.speech_end_at {
                                        let ms = t0.elapsed().as_millis() as u64;
                                        self.metrics.lock().last_time_to_first_audio_ms = Some(ms);
                                        tracing::info!(ms, "voice time to first audio");
                                    }
                                }
                            }
                            Err(e) => tracing::warn!("speech synthesis failed: {e}"),
                        }
                        pl.refresh_state(&vad);
                    }
                    Ev::Audio(..) => {}
                },
                _ = level_tick.tick() => {
                    let _ = self.app.emit(EVT_LEVEL, LevelEvent { input: audio.levels.input(), output: audio.levels.output() });
                    pl.refresh_state(&vad);
                }
            }
        };

        if pl.turn_active {
            self.app.state::<Arc<Agent>>().cancel();
        }
        audio.stop();
        // Keep the model warm for a quick restart, then free the memory.
        let me = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(UNLOAD_AFTER + Duration::from_secs(1)).await;
            if !me.is_active() {
                let engine = me.stt.clone();
                let _ = tokio::task::spawn_blocking(move || engine.unload_if_idle()).await;
            }
        });
        // A failed preload surfaces as the session error.
        if let Ok(Err(e)) = preload.await {
            return Err(e);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_words() {
        assert!(is_stop("Стоп."));
        assert!(is_stop("Джарвис, хватит!"));
        assert!(!is_stop("Стоп, открой калькулятор"));
        assert!(!is_stop("Открой браузер"));
    }
}
