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
use tauri::{Emitter, Listener, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::audio::{AudioIo, SPEAKER_RATE};
use super::phrases::PhraseSplitter;
use super::models;
use super::stt::{self, Recognizer, Transcript};
use super::tts::Speaker;
use super::vad::{Vad, VadEvent};
use super::{Ctl, LevelEvent, TranscriptEvent, VoiceManager, VoiceState, EVT_LEVEL, EVT_TRANSCRIPT};
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

/// Speech that resumes this soon after the VAD closed the previous phrase
/// continues it: the actual pause was under ~2 s (the VAD reports the end
/// 0.9 s after the last word and the start 0.25 s after the first). Live
/// speech pauses between sentences longer than the end-of-utterance silence.
const CONTINUATION_GAP: Duration = Duration::from_millis(1300);

/// A conversation with no speech for this long goes back to waiting for
/// the name (when that is on).
const SLEEP_AFTER: Duration = Duration::from_secs(60);

/// What JARVIS says when called by name alone.
fn acknowledgement(vy: bool) -> String {
    let options: &[&str] = if vy { &["Слушаю вас.", "Да?", "Я здесь."] } else { &["Слушаю.", "Да?", "Я здесь.", "Слушаю тебя."] };
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(0);
    options[n % options.len()].to_string()
}

/// GigaAM writes a lone «стоп» in Latin («Stop.») — seen live.
const STOP_WORDS: &[&str] = &["стоп", "хватит", "отмена", "отмени", "замолчи", "тихо", "стой", "stop"];

fn is_stop(text: &str) -> bool {
    let t: String = text.to_lowercase().chars().filter(|c| c.is_alphabetic() || c.is_whitespace()).collect();
    let words: Vec<&str> = t.split_whitespace().filter(|w| !super::wake::is_name(w)).collect();
    !words.is_empty() && words.len() <= 2 && words.iter().all(|w| STOP_WORDS.contains(w))
}

enum Ev {
    /// Result, when the phrase ended, whether it continues the previous one.
    Transcribed(AppResult<Transcript>, Instant, bool),
    Delta(u64, String),
    TurnDone(u64, AppResult<String>),
    Audio(u64, AppResult<Vec<i16>>),
}

struct Pipeline {
    vm: Arc<VoiceManager>,
    /// In a conversation; otherwise only listening for the name.
    awake: bool,
    wake_enabled: bool,
    address_vy: bool,
    last_activity: Instant,
    /// «Стоп» without the name listener: close the session.
    ending: bool,
    audio: Arc<AudioIo>,
    ev_tx: mpsc::UnboundedSender<Ev>,
    synth_tx: mpsc::UnboundedSender<(u64, String)>,
    model_path: PathBuf,
    transcribing: usize,
    turn_seq: u64,
    turn_active: bool,
    /// What the running turn was asked (phrase merging).
    turn_text: String,
    /// When the VAD closed the last phrase, and whether the phrase being
    /// heard now continues it.
    last_ended: Option<Instant>,
    hearing_continues: bool,
    turn_finished: Option<Instant>,
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

    fn wake(&mut self) {
        if !self.awake {
            self.awake = true;
            self.vm.set_awake(true);
            tracing::info!("voice: conversation started");
        }
        self.last_activity = Instant::now();
    }

    /// Back to waiting for the name: silence, cancel the running turn.
    fn sleep(&mut self) {
        self.hush();
        self.pending_request = None;
        if self.turn_active {
            self.vm.app.state::<Arc<Agent>>().cancel();
        }
        if self.awake {
            self.awake = false;
            self.vm.set_awake(false);
            tracing::info!("voice: waiting for the name");
        }
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
        self.turn_text = text.clone();
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

    fn on_request(&mut self, text: String, continues: bool) {
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        if is_stop(&text) {
            // «Стоп», «хватит»: silence, cancel, and end the voice mode —
            // back to waiting for the name, or microphone off.
            tracing::info!(busy = agent.is_busy(), "stop word");
            self.hush();
            if agent.is_busy() {
                agent.cancel();
            }
            self.pending_request = None;
            if self.wake_enabled {
                self.sleep();
            } else {
                self.ending = true;
            }
            return;
        }
        if self.turn_active && continues {
            // The user paused mid-thought and went on: one request, not two
            // answers (their speech already hushed any reply).
            tracing::info!("phrase merged into the running voice turn");
            self.pending_request = Some(format!("{} {}", self.turn_text, text));
            agent.cancel();
        } else if continues && !self.turn_active && !self.turn_text.is_empty() && self.turn_finished.is_some_and(|t| t.elapsed() < Duration::from_secs(6)) {
            // The answer came while the user was still talking, and their
            // speech cut it off: ask again with the whole thought.
            tracing::info!("phrase merged with the just-answered one");
            self.hush();
            let whole = format!("{} {}", self.turn_text, text);
            self.start_turn(whole);
        } else if self.turn_active {
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
        if !self.awake {
            VoiceState::Dormant
        } else if vad.is_active() {
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
    /// `dormant`: start by waiting for the name (Settings → «Откликаться на имя»).
    pub(super) async fn run_local(
        self: &Arc<Self>,
        cfg: VoiceSettings,
        cancel: CancellationToken,
        mut ctl_rx: mpsc::UnboundedReceiver<Ctl>,
        dormant: bool,
    ) -> AppResult<()> {
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
        self.listener_failures.store(0, std::sync::atomic::Ordering::Relaxed);
        let initial = if dormant { VoiceState::Dormant } else { VoiceState::Listening };
        self.set_state(initial, None, Some((&audio.input_device, &audio.output_device)));
        tracing::info!(voice = %speaker.name(), dormant, "local voice session started");
        let address_vy = store.settings().persona.address == "vy";

        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<Ev>();
        // Phrases are synthesised one at a time, in order.
        let (synth_tx, mut synth_rx) = mpsc::unbounded_channel::<(u64, String)>();
        {
            let ev_tx = ev_tx.clone();
            let speaker = speaker.clone();
            tauri::async_runtime::spawn(async move {
                while let Some((gen, phrase)) = synth_rx.recv().await {
                    let sp = speaker.clone();
                    let pcm = tokio::task::spawn_blocking(move || {
                        // A breath after each sentence, longer after «?» and «…».
                        sp.synthesize(&phrase, SPEAKER_RATE).map(|mut pcm| {
                            let pause = super::ru_speech::pause_after_ms(&phrase) as usize * SPEAKER_RATE as usize / 1000;
                            pcm.resize(pcm.len() + pause, 0);
                            pcm
                        })
                    })
                    .await
                    .unwrap_or_else(|e| Err(AppError::internal(e.to_string())));
                    let _ = ev_tx.send(Ev::Audio(gen, pcm));
                }
            });
        }

        let mut pl = Pipeline {
            vm: Arc::clone(self),
            awake: !dormant,
            wake_enabled: cfg.wake_word,
            address_vy,
            last_activity: Instant::now(),
            ending: false,
            audio: audio.clone(),
            ev_tx: ev_tx.clone(),
            synth_tx,
            model_path,
            transcribing: 0,
            turn_seq: 0,
            turn_active: false,
            turn_text: String::new(),
            last_ended: None,
            hearing_continues: false,
            turn_finished: None,
            pending_request: None,
            splitter: PhraseSplitter::default(),
            reply: String::new(),
            spoken_any: false,
            speech_gen: 0,
            queued_phrases: 0,
            speech_end_at: None,
            first_audio_pending: false,
            shown: initial,
        };
        let mut level_tick = tokio::time::interval(Duration::from_millis(33));
        // A voice user is not looking at the screen: say it aloud when an
        // action waits for confirmation (given only by clicking on the island —
        // a misheard «да» must not send a message).
        let (confirm_tx, mut confirm_rx) = mpsc::unbounded_channel::<String>();
        let confirm_listener = self.app.listen(crate::agent::events::EVT_AGENT, move |e| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(e.payload()) {
                if v["type"] == "confirmationRequired" {
                    let _ = confirm_tx.send(v["title"].as_str().unwrap_or_default().to_string());
                }
            }
        });

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
                            pl.hearing_continues = pl.last_ended.is_some_and(|e| e.elapsed() < CONTINUATION_GAP);
                            if pl.awake {
                                pl.last_activity = Instant::now();
                            }
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
                            pl.last_ended = Some(ended);
                            let continues = std::mem::take(&mut pl.hearing_continues);
                            let engine = self.stt.clone();
                            let path = pl.model_path.clone();
                            let tx = ev_tx.clone();
                            tokio::task::spawn_blocking(move || {
                                let res = engine.transcribe(&path, &utterance);
                                let _ = tx.send(Ev::Transcribed(res, ended, continues));
                            });
                        }
                    } }
                    pl.refresh_state(&vad);
                }
                Some(ev) = ev_rx.recv() => match ev {
                    Ev::Transcribed(res, ended, continues) => {
                        pl.transcribing = pl.transcribing.saturating_sub(1);
                        match res {
                            Ok(t) => {
                                self.metrics.lock().last_stt_ms = Some(t.ms);
                                let words = t.text.split_whitespace().count();
                                tracing::info!(ms = t.ms, words, "speech recognised");
                                if !stt::is_meaningful(&t.text) {
                                    tracing::debug!("ignored non-speech");
                                } else if !pl.awake {
                                    // Waiting for the name: phrases not addressed to JARVIS
                                    // are dropped here — not shown, logged or sent.
                                    match super::wake::addressed(&t.text) {
                                        Some(a) => {
                                            tracing::info!(only_name = a.only_name, "voice: called by name");
                                            pl.wake();
                                            pl.emit_transcript("user", &t.text, true);
                                            if a.only_name {
                                                let ack = acknowledgement(pl.address_vy);
                                                pl.say(ack);
                                            } else {
                                                pl.speech_end_at = Some(ended);
                                                pl.first_audio_pending = true;
                                                pl.on_request(t.text, false);
                                            }
                                        }
                                        None => tracing::debug!(words, "voice: not addressed"),
                                    }
                                } else {
                                    pl.emit_transcript("user", &t.text, true);
                                    pl.speech_end_at = Some(ended);
                                    pl.first_audio_pending = true;
                                    pl.on_request(t.text, continues);
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
                        pl.turn_finished = Some(Instant::now());
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
                Some(title) = confirm_rx.recv() => {
                    if pl.turn_active {
                        pl.say(format!("Нужно ваше подтверждение: {}. Нажмите «Разрешить» или «Отклонить» на острове.", super::phrases::speakable(&title)));
                        pl.refresh_state(&vad);
                    }
                }
                Some(c) = ctl_rx.recv() => {
                    match c {
                        Ctl::Wake => pl.wake(),
                        Ctl::Sleep => pl.sleep(),
                    }
                    pl.refresh_state(&vad);
                }
                _ = level_tick.tick() => {
                    // Levels only drive the open voice view; none while waiting.
                    if pl.awake {
                        let _ = self.app.emit(EVT_LEVEL, LevelEvent { input: audio.levels.input(), output: audio.levels.output() });
                        if pl.audio.is_playing() || pl.turn_active || pl.transcribing > 0 || pl.queued_phrases > 0 {
                            pl.last_activity = Instant::now();
                        } else if pl.wake_enabled && !vad.is_active() && pl.last_activity.elapsed() >= SLEEP_AFTER {
                            pl.sleep();
                        }
                    }
                    if pl.ending {
                        break Ok(());
                    }
                    pl.refresh_state(&vad);
                }
            }
        };

        self.app.unlisten(confirm_listener);
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
        assert!(is_stop("Stop."));
        assert!(is_stop("Джар, стоп!"));
        assert!(is_stop("Джарвис, хватит!"));
        assert!(!is_stop("Стоп, открой калькулятор"));
        assert!(!is_stop("Открой браузер"));
    }
}
