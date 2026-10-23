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

    pub(super) fn transcribe(&self, path: &Path, pcm: &[f32]) -> AppResult<Transcript> {
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

fn continues_thought(gap:Duration,awaiting_first_audio:bool)->bool { awaiting_first_audio && gap<CONTINUATION_GAP }

/// A conversation with no speech for this long goes back to waiting for
/// the name (when that is on).
const SLEEP_AFTER: Duration = Duration::from_secs(60);

/// What JARVIS says when called by name alone.
fn acknowledgement(vy: bool) -> String {
    if vy { "Слушаю вас." } else { "Слушаю." }.into()
}

/// Said at once when the answer is slow to start, so the pause after a
/// request is never dead air. Cached at session start: they cost nothing.
const FILLERS: &[&str] = &["Одну секунду.", "Ага.", "Сейчас.", "Минутку."];

/// How long a turn may stay silent before a filler is said.
const FILLER_AFTER: Duration = Duration::from_millis(700);

/// GigaAM writes a lone «стоп» in Latin («Stop.») — seen live.
const STOP_WORDS: &[&str] = &["стоп", "хватит", "отмена", "отмени", "замолчи", "тихо", "стой", "stop"];

fn is_stop(text: &str) -> bool {
    let t: String = text.to_lowercase().chars().filter(|c| c.is_alphabetic() || c.is_whitespace()).collect();
    let words: Vec<&str> = t.split_whitespace().filter(|w| !super::wake::is_name(w)).collect();
    !words.is_empty() && words.len() <= 2 && words.iter().all(|w| STOP_WORDS.contains(w))
}

/// Words that cut JARVIS off without its name.
const INTERRUPT_WORDS: &[&str] = &["подожди", "погоди", "постой", "секунду", "нет", "не", "отмена", "тише", "замолчи", "wait"];

/// Speech heard while JARVIS talks. Through speakers the microphone also
/// hears JARVIS itself («Слушаю вас.» came back as a «command»), so only the
/// user's own signals count: a stop word, the name, «подожди…», or the rest of
/// the thought they were saying. In headphones there is no echo: any words do.
fn is_reply_interruption(text: &str, continues: bool, headphones: bool) -> bool {
    if !stt::is_meaningful(text) {
        return false;
    }
    if headphones || continues || is_stop(text) || super::wake::addressed(text).is_some() {
        return true;
    }
    let first: String = text.to_lowercase().chars().take_while(|c| c.is_alphabetic()).collect();
    INTERRUPT_WORDS.contains(&first.as_str())
}

/// Silence after a conversation ended without the name listener: the
/// devices stay open this long for an instant restart, then close.
const REST_FOR: Duration = Duration::from_secs(180);

/// While waiting for the name, long phrases (TV, a call, a talk in the room)
/// are checked by their opening only: the name must open a call, and a full
/// decode of 20 s of someone else's speech took 5 s and delayed the next
/// «Джарвис».
const DORMANT_FULL_DECODE: usize = 16_000 * 4;
const DORMANT_HEAD: usize = 16_000 * 3;

enum Ev {
    /// Result, when the phrase ended, whether it continues the previous one.
    Transcribed(AppResult<Transcript>, Instant, bool),
    Delta(u64, String),
    TurnDone(u64, AppResult<String>),
    /// Part of a phrase, as a streaming voice makes it.
    AudioChunk(u64, Vec<i16>),
    /// The phrase is complete: its tail (the pause after it), or the error.
    Audio(u64, AppResult<Vec<i16>>),
    WakePreview(u64, AppResult<Transcript>),
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
    synth_generation:Arc<std::sync::atomic::AtomicU64>,
    queued_phrases: usize,
    speech_end_at: Option<Instant>,
    first_audio_pending: bool,
    shown: VoiceState,
    utterance_seq: u64,
    preview_job: Option<u64>,
    hearing_since: Option<Instant>,
    playback_pending: bool,
    /// Speech generation whose only queued line is «Слушаю»: words spoken
    /// over it are the command it invited, not an interruption.
    ack_gen: Option<u64>,
    /// Ended without the name listener: hear nothing, close after `REST_FOR`.
    resting_since: Option<Instant>,
    /// When to say a filler if the answer hasn't started by then.
    filler_at: Option<Instant>,
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
        self.resting_since = None;
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
        self.utterance_seq += 1;
        if self.turn_active {
            self.vm.app.state::<Arc<Agent>>().cancel();
        }
        if self.awake {
            self.awake = false;
            self.vm.set_awake(false);
            tracing::info!("voice: waiting for the name");
        }
    }

    /// The conversation is over and nobody listens for the name: keep the
    /// devices warm, hear nothing.
    fn rest(&mut self) {
        self.sleep();
        self.resting_since = Some(Instant::now());
        tracing::info!("voice: resting, microphone kept open without listening");
    }

    /// The acknowledgement after the bare name is the only thing playing.
    fn ack_only(&self) -> bool {
        self.ack_gen == Some(self.speech_gen) && !self.turn_active
    }

    /// Drop everything queued or playing.
    fn hush(&mut self) {
        self.ack_gen = None;
        self.filler_at = None;
        self.audio.clear_playback();
        self.speech_gen += 1;
        self.synth_generation.store(self.speech_gen,std::sync::atomic::Ordering::Release);
        self.queued_phrases = 0;
        self.playback_pending = false;
    }

    fn start_turn(&mut self, text: String) {
        self.turn_seq += 1;
        self.ack_gen = None;
        self.turn_active = true;
        self.turn_text = text.clone();
        self.splitter = PhraseSplitter::default();
        self.reply.clear();
        self.spoken_any = false;
        self.filler_at = Some(Instant::now() + FILLER_AFTER);
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
                self.rest();
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

        let (voice, rate) = (cfg.tts_voice.clone(), cfg.tts_rate);
        let speech=self.speech.clone();
        let speaker = tokio::task::spawn_blocking(move || {
            speech.get(&voice,rate)
        })
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
        let (input,output)=(cfg.input_device.clone(),cfg.output_device.clone());
        let opening=tokio::task::spawn_blocking(move || AudioIo::start(&input,&output));
        let (audio, mut mic) = tokio::select! {
            _=cancel.cancelled()=>return Err(AppError::Cancelled),
            res=opening=>res.map_err(|e|AppError::internal(e.to_string()))??,
        };
        self.listener_failures.store(0, std::sync::atomic::Ordering::Relaxed);
        let initial = if dormant { VoiceState::Dormant } else { VoiceState::Listening };
        self.set_state(initial, None, Some((&audio.input_device, &audio.output_device)));
        tracing::info!(voice = %speaker.speaker.name(), dormant, "local voice session started");
        let address_vy = store.settings().persona.address == "vy";
        let ready_speaker=speaker.clone();
        tokio::task::spawn_blocking(move || {
            let _=ready_speaker.synthesize(&acknowledgement(address_vy));
            for f in FILLERS { let _=ready_speaker.synthesize(f); }
        });

        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<Ev>();
        // Phrases are synthesised one at a time, in order.
        let (synth_tx, mut synth_rx) = mpsc::unbounded_channel::<(u64, String)>();
        let synth_generation=Arc::new(std::sync::atomic::AtomicU64::new(0));
        {
            let ev_tx = ev_tx.clone();
            let speaker = speaker.clone();
            let generation=synth_generation.clone();
            tauri::async_runtime::spawn(async move {
                while let Some((gen, phrase)) = synth_rx.recv().await {
                    if gen!=generation.load(std::sync::atomic::Ordering::Acquire) { continue; }
                    let sp = speaker.clone();
                    let (chunks, live) = (ev_tx.clone(), generation.clone());
                    let pcm = tokio::task::spawn_blocking(move || {
                        // Streaming voices start playing before the phrase is
                        // done; a hushed phrase stops downloading.
                        sp.synthesize_stream(&phrase, &mut |pcm| {
                            if gen != live.load(std::sync::atomic::Ordering::Acquire) {
                                return false;
                            }
                            chunks.send(Ev::AudioChunk(gen, pcm.to_vec())).is_ok()
                        })
                        // A breath after each sentence, longer after «?» and «…».
                        .map(|()| vec![0; super::ru_speech::pause_after_ms(&phrase) as usize * SPEAKER_RATE as usize / 1000])
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
            synth_generation,
            queued_phrases: 0,
            speech_end_at: None,
            first_audio_pending: false,
            shown: initial,
            utterance_seq:0,preview_job:None,hearing_since:None,
            playback_pending:false,
            filler_at: None,
            ack_gen: None,
            resting_since: None,
        };
        let mut level_tick = tokio::time::interval(Duration::from_millis(33));
        let mut last_mic=Instant::now();
        let mut stale_seen=0;
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
                chunk = mic.recv() => {
                    let Some(chunk) = chunk else { break Err(AppError::platform("Микрофон перестал отвечать")) };
                    last_mic=Instant::now();
                    if mic.stale_chunks!=stale_seen {
                        tracing::warn!(dropped=mic.stale_chunks-stale_seen,"discarded delayed microphone audio");
                        stale_seen=mic.stale_chunks;
                        vad.reset();
                    }
                    let pcm: Vec<f32> = chunk.iter().map(|s| *s as f32 / 32768.0).collect();
                    // Our own voice through speakers must be louder to count.
                    vad.echo_guard = pl.audio.is_playing() && !pl.audio.headphones;
                    vad.wake_preview = !pl.awake && pl.wake_enabled && pl.resting_since.is_none();
                    for event in vad.push(&pcm) { match event {
                        VadEvent::Started => {
                            pl.utterance_seq += 1;
                            pl.hearing_since=Some(Instant::now());
                            pl.hearing_continues = pl.last_ended.is_some_and(|e| continues_thought(e.elapsed(),pl.first_audio_pending));
                            if pl.awake {
                                pl.last_activity = Instant::now();
                            }
                            // VAD is only a candidate: breaths and room noise
                            // must not destroy a spoken answer. Confirm below
                            // after ASR returns meaningful words.
                        }
                        // Resting: the microphone is open only to restart fast.
                        VadEvent::Ended(_) if !pl.awake && (!pl.wake_enabled || pl.resting_since.is_some()) => {}
                        VadEvent::Ended(utterance) => {
                            pl.transcribing += 1;
                            let ended = Instant::now();
                            pl.last_ended = Some(ended);
                            let continues = std::mem::take(&mut pl.hearing_continues);
                            let engine = self.stt.clone();
                            let path = pl.model_path.clone();
                            let tx = ev_tx.clone();
                            let dormant = !pl.awake;
                            tokio::task::spawn_blocking(move || {
                                let res = if dormant && utterance.len() > DORMANT_FULL_DECODE {
                                    // Only a call by name matters here: decode the
                                    // opening, and the rest only if it is one.
                                    match engine.transcribe(&path, &utterance[..DORMANT_HEAD]) {
                                        Ok(head) if super::wake::addressed(&head.text).is_none() => Ok(head),
                                        _ => engine.transcribe(&path, &utterance),
                                    }
                                } else {
                                    engine.transcribe(&path, &utterance)
                                };
                                let _ = tx.send(Ev::Transcribed(res, ended, continues));
                            });
                        }
                        VadEvent::Preview(pcm) if !pl.awake && pl.wake_enabled && pl.preview_job.is_none() => {
                            let seq=pl.utterance_seq;
                            pl.preview_job=Some(seq);
                            let engine=self.stt.clone();
                            let path=pl.model_path.clone();
                            let tx=ev_tx.clone();
                            tokio::task::spawn_blocking(move || { let res=engine.transcribe(&path,&pcm);let _=tx.send(Ev::WakePreview(seq,res)); });
                        }
                        VadEvent::Preview(_) => {}
                    } }
                    pl.refresh_state(&vad);
                }
                Some(ev) = ev_rx.recv() => match ev {
                    Ev::WakePreview(seq,res) => {
                        if pl.preview_job==Some(seq) { pl.preview_job=None; }
                        if !pl.awake && seq==pl.utterance_seq && res.ok().is_some_and(|t| super::wake::addressed(&t.text).is_some()) {
                            let ms=pl.hearing_since.map(|t|t.elapsed().as_millis() as u64).unwrap_or(0);
                            tracing::info!(ms,"voice: early name detected");
                            self.metrics.lock().last_wake_ms=Some(ms);
                            pl.wake();
                            // Heard: a short cue now, the answer after the command.
                            pl.audio.play_pcm16(&AudioIo::chime());
                            pl.refresh_state(&vad);
                        }
                    }
                    Ev::Transcribed(res, ended, continues) => {
                        pl.transcribing = pl.transcribing.saturating_sub(1);
                        match res {
                            Ok(t) => {
                                self.metrics.lock().last_stt_ms = Some(t.ms);
                                let words = t.text.split_whitespace().count();
                                tracing::info!(ms = t.ms, words, "speech recognised");
                                let meaningful = stt::is_meaningful(&t.text);
                                // Words over «Слушаю» (or the wake cue) are the command itself.
                                let invited = pl.ack_only();
                                let reply_busy = !invited && (pl.audio.is_playing() || pl.queued_phrases > 0);
                                if invited && meaningful {
                                    pl.hush();
                                }
                                if pl.awake && reply_busy && !is_reply_interruption(&t.text, continues, pl.audio.headphones) {
                                    tracing::info!("ignored non-speech during reply");
                                    pl.refresh_state(&vad);
                                    continue;
                                }
                                if meaningful && pl.awake && reply_busy {
                                    let t = Instant::now();
                                    pl.hush();
                                    let ms = t.elapsed().as_millis() as u64;
                                    self.metrics.lock().last_barge_in_ms = Some(ms);
                                    tracing::info!(ms, "barge-in: speech cleared after recognised words");
                                }
                                if !meaningful {
                                    if reply_busy { tracing::info!("ignored non-speech during reply"); }
                                    tracing::debug!("ignored non-speech");
                                } else if super::wake::addressed(&t.text).is_some_and(|a|a.only_name) {
                                    pl.wake();
                                    pl.emit_transcript("user",&t.text,true);
                                    pl.say(acknowledgement(pl.address_vy));
                                    pl.ack_gen = Some(pl.speech_gen);
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
                                                pl.ack_gen = Some(pl.speech_gen);
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
                        pl.filler_at = None;
                        pl.reply.push_str(&d);
                        pl.emit_transcript("assistant", &d, false);
                        for phrase in pl.splitter.push(&d) {
                            pl.say(phrase);
                        }
                    }
                    Ev::Delta(..) => {}
                    Ev::TurnDone(seq, res) if seq == pl.turn_seq => {
                        pl.turn_active = false;
                        pl.filler_at = None;
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
                    Ev::AudioChunk(gen, pcm) if gen == pl.speech_gen => {
                        pl.audio.play_pcm16(&pcm);
                        pl.playback_pending = true;
                        if pl.first_audio_pending {
                            pl.first_audio_pending = false;
                            if let Some(t0) = pl.speech_end_at {
                                let ms = t0.elapsed().as_millis() as u64;
                                self.metrics.lock().last_time_to_first_audio_ms = Some(ms);
                                tracing::info!(ms, "voice time to first audio");
                            }
                        }
                        pl.refresh_state(&vad);
                    }
                    Ev::AudioChunk(..) => {}
                    Ev::Audio(gen, res) if gen == pl.speech_gen => {
                        pl.queued_phrases = pl.queued_phrases.saturating_sub(1);
                        match res {
                            Ok(pcm) => {
                                pl.audio.play_pcm16(&pcm);
                                pl.playback_pending = true;
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
                        Ctl::Rest => pl.rest(),
                        Ctl::Say(line) => pl.say(line),
                    }
                    pl.refresh_state(&vad);
                }
                _ = level_tick.tick() => {
                    if pl.turn_active && pl.filler_at.is_some_and(|t| Instant::now() >= t) {
                        pl.filler_at = None;
                        if pl.queued_phrases == 0 && !pl.audio.is_playing() {
                            // Not `say`: a filler doesn't count as the answer.
                            pl.queued_phrases += 1;
                            let line = FILLERS[pl.turn_seq as usize % FILLERS.len()].to_string();
                            let _ = pl.synth_tx.send((pl.speech_gen, line));
                        }
                    }
                    if pl.playback_pending && !pl.turn_active && pl.queued_phrases == 0 && !pl.audio.is_playing() {
                        tracing::info!(turn=pl.turn_seq,"voice playback drained");
                        pl.playback_pending = false;
                    }
                    if last_mic.elapsed()>Duration::from_secs(5) {
                        break Err(AppError::platform("Звук с микрофона перестал поступать. Переподключаю слушатель."));
                    }
                    // Levels only drive the open voice view; none while waiting.
                    if pl.awake {
                        let _ = self.app.emit(EVT_LEVEL, LevelEvent { input: audio.levels.input(), output: audio.levels.output() });
                        if pl.audio.is_playing() || pl.turn_active || pl.transcribing > 0 || pl.queued_phrases > 0 {
                            pl.last_activity = Instant::now();
                        } else if pl.wake_enabled && !vad.is_active() && pl.last_activity.elapsed() >= SLEEP_AFTER {
                            pl.sleep();
                        }
                    }
                    if pl.ending || pl.resting_since.is_some_and(|t| t.elapsed() >= REST_FOR) {
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
            if me.session.lock().is_none() {
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
    fn background_speech_cannot_interrupt_but_stop_name_and_continuation_can() {
        for text in ["", "Хм.", "Слушаю вас.", "Сегодня хорошая погода."] {
            assert!(!is_reply_interruption(text, false, false), "unaddressed background interrupted: {text}");
        }
        for text in ["Стоп!", "Джарвис, подожди.", "Эй, Джар, какая громкость?"] {
            assert!(is_reply_interruption(text, false, false), "explicit interruption ignored: {text}");
        }
        assert!(is_reply_interruption("и яркость", true, false));
        assert!(!is_reply_interruption("", true, false));
        assert!(is_reply_interruption("Подожди, не то.", false, false));
        // Headphones: no echo, plain words interrupt; sounds still do not.
        assert!(is_reply_interruption("Сегодня хорошая погода.", false, true));
        assert!(!is_reply_interruption("Хм.", false, true));
    }

    #[test]
    fn stop_words() {
        assert!(is_stop("Стоп."));
        assert!(is_stop("Stop."));
        assert!(is_stop("Джар, стоп!"));
        assert!(is_stop("Джарвис, хватит!"));
        assert!(!is_stop("Стоп, открой калькулятор"));
        assert!(!is_stop("Открой браузер"));
    }

    #[test]
    fn a_new_command_after_a_spoken_answer_is_not_replayed_with_the_previous_action() {
        assert!(continues_thought(Duration::from_millis(500),true));
        assert!(!continues_thought(Duration::from_millis(500),false));
        assert!(!continues_thought(Duration::from_secs(3),true));
    }
}
