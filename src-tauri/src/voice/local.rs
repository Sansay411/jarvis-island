//! Local voice pipeline: microphone → Silero VAD → GigaAM (sherpa-onnx) →
//! the text agent (same model, tools, confirmations and chat history as typed
//! requests) → neural speech synthesis → speakers.
//!
//! Speech starts with the first complete sentence of the reply. Speaking
//! over JARVIS clears queued speech at once (barge-in). A new request while a
//! voice turn is still running waits for it (the latest one wins); only
//! «стоп»/«хватит» cancel — live use showed that cancelling on every remark
//! («спасибо») killed long tasks.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::{Emitter, Listener, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::audio::{AudioIo, SPEAKER_RATE};
use super::intent::{self, is_stop, Intent};
use super::phrases::PhraseSplitter;
use super::models;
use super::stt::{self, Recognizer, Transcript};
use super::vad::{Vad, VadEvent};
use super::{Ctl, LevelEvent, TranscriptEvent, VoiceManager, VoiceState, EVT_LEVEL, EVT_TRANSCRIPT};
use crate::agent::{Agent, Heard};
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

/// An utterance whose words have not changed for this long has ended, even
/// if background sound keeps the detector open.
const STABLE_FOR: Duration = Duration::from_millis(1300);

/// A conversation with nothing going on for this long goes back to waiting
/// for the name (when that is on). Counted from the end of JARVIS's own
/// work and speech — the room's voices do not keep it open: with a film
/// on, every line of dialogue used to be a command for the next minute.
/// 25 s still let a song and a talk at home be answered phrase after
/// phrase (2026-11-20/21): a follow-up comes within seconds or not at all.
const SLEEP_AFTER: Duration = Duration::from_secs(12);

/// After «Слушаю» (or a click, or the shortcut) the next words are the
/// command, whatever they are — for this long.
const INVITE_FOR: Duration = Duration::from_secs(10);

/// After the user's own media command, «дальше… дальше…» works without the
/// name for this long, even with the music playing.
const MEDIA_CHAIN: Duration = Duration::from_secs(10);

/// How loud other apps stay while JARVIS listens and talks (≈ −10 dB).
const DUCK_TO: f32 = 0.3;
/// Where the levels to restore are kept until restored: a crash must not
/// leave the user's music quiet for good (Windows remembers app levels).
const DUCK_KEY: &str = "voice.ducked";

/// What the full phrase says when the early cue heard the name in its
/// opening but the final decode has none.
#[derive(Debug, PartialEq)]
enum LostName {
    /// «Джас.»: the mangled name alone — the call itself.
    Call,
    /// «Джас. Включи музыку.»: the command after it, the name put back.
    Command(String),
    /// «Дарья, привет»: the cue misheard the opening; no call at all.
    NotACall,
}

fn after_lost_name(text: &str) -> LostName {
    let w = super::wake::spans(text);
    let Some((_, first)) = w.first() else { return LostName::NotACall };
    if !super::wake::near_name(first) {
        return LostName::NotACall;
    }
    match w.get(1) {
        None => LostName::Call,
        Some((at, _)) => LostName::Command(format!("Джарвис, {}", text[*at..].trim())),
    }
}

/// What JARVIS says when called by name alone.
fn acknowledgement(vy: bool) -> String {
    if vy { "Слушаю вас." } else { "Слушаю." }.into()
}

/// The answer to a call by name: a greeting is greeted back (instantly, no
/// model) — «Привет, Джарвис» answered with «Слушаю.» five times in a row
/// on 2026-11-18 while the user kept saying hello.
fn answer_call(text: &str, vy: bool) -> String {
    if super::wake::greets(text) {
        if vy { "Здравствуйте! Чем могу помочь?" } else { "Привет! Чем помочь?" }.into()
    } else {
        acknowledgement(vy)
    }
}

/// Said at once when the answer is slow to start, so the pause after a
/// request is never dead air. Cached at session start: they cost nothing.
const FILLERS: &[&str] = &["Одну секунду.", "Ага.", "Сейчас.", "Минутку."];

/// How long a turn may stay silent before a filler is said.
const FILLER_AFTER: Duration = Duration::from_millis(1500);

/// Words that cut JARVIS off without its name (when no task is running —
/// see `intent::classify` for what counts while one is).
const INTERRUPT_WORDS: &[&str] = &["подожди", "погоди", "постой", "секунду", "нет", "не", "отмена", "тише", "замолчи", "wait"];

/// Speech heard while JARVIS talks. Through speakers the microphone also
/// hears JARVIS itself («Слушаю вас.» came back as a «command»), so only the
/// user's own signals count: a stop word, the name, «подожди…», or the rest of
/// the thought they were saying. In headphones there is no echo: any words do.
fn is_reply_interruption(text: &str, continues: bool, headphones: bool) -> bool {
    if !stt::is_meaningful(text) {
        return false;
    }
    if headphones || continues || is_stop(text) || super::wake::addressed(text).is_some() || super::intent::interrupts_reply(text) {
        return true;
    }
    let first: String = text.to_lowercase().chars().take_while(|c| c.is_alphabetic()).collect();
    INTERRUPT_WORDS.contains(&first.as_str())
}

/// A held reply waits at least this long for the words, and at most this
/// long while sound goes on.
const HOLD_MIN: Duration = Duration::from_millis(350);
const HOLD_MAX: Duration = Duration::from_secs(8);

/// Most of the recognised words are JARVIS's own recent lines.
fn is_echo(heard: &str, said: &VecDeque<String>) -> bool {
    let words = |t: &str| t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() > 2).map(str::to_string).collect::<Vec<_>>();
    let heard = words(heard);
    if heard.is_empty() {
        return false;
    }
    let said: std::collections::HashSet<String> = said.iter().flat_map(|l| words(l)).collect();
    let hits = heard.iter().filter(|w| said.contains(*w)).count();
    hits * 10 >= heard.len() * 6
}

/// A phrase that repeats the running request this soon is the recogniser
/// hearing one utterance twice, not the user asking again.
const REPEAT_WINDOW: Duration = Duration::from_secs(5);

/// `heard` is the running request again (the request may carry a
/// «[Пользователь перебил …]» note in front, so the end is compared).
fn same_words(heard: &str, request: &str) -> bool {
    let norm = |t: &str| t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let (h, r) = (norm(heard), norm(request));
    !h.is_empty() && (h == r || r.ends_with(&format!(" {h}")))
}

/// The answer asks the user something back, so the microphone stays open for
/// the reply. Anything else closes the exchange.
fn asks_back(answer: &str) -> bool {
    answer.trim_end().trim_end_matches(|c: char| c == ')' || c == '»' || c == '"').ends_with('?')
}

fn short(t: &str, max: usize) -> String {
    let t = t.trim();
    if t.chars().count() <= max { t.to_string() } else { format!("{}…", t.chars().take(max).collect::<String>()) }
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
    /// Result, when the phrase ended, whether it continues the previous one,
    /// and which utterance it was.
    Transcribed(AppResult<Transcript>, Instant, bool, u64),
    Delta(u64, String),
    TurnDone(u64, AppResult<String>),
    /// Part of a phrase, as a streaming voice makes it.
    AudioChunk(u64, Vec<i16>),
    /// The phrase is complete: its tail (the pause after it), or the error.
    Audio(u64, AppResult<Vec<i16>>),
    WakePreview(u64, AppResult<Transcript>),
    /// A decode of the utterance so far (awake), for endpointing.
    Progress(u64, AppResult<Transcript>),
    /// Apps made quieter for the conversation.
    Ducked(AppResult<Vec<crate::platform::audio::Ducked>>),
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
    /// Finished requests also go to SpeechKit through Jarvis Cloud.
    cloud_stt: bool,
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
    /// The next request, and how sure it is to be for JARVIS.
    pending_request: Option<(String, Heard)>,
    /// How sure the running turn is to be for JARVIS. When unsure the model
    /// may answer it with silence; until it says something, nothing is shown
    /// or said.
    turn_heard: Heard,
    ambient_line: Option<String>,
    /// The next words are the command, whatever they are, until then
    /// («Слушаю», a click on the island, the shortcut).
    invite_until: Option<Instant>,
    /// The utterance whose opening woke JARVIS (early name cue).
    woke_seq: Option<u64>,
    /// When the user's last media command was taken.
    last_media_at: Option<Instant>,
    /// JARVIS's last answer (did it end with a question?).
    last_answer: String,
    /// Go back to waiting for the name once the line now queued is said.
    sleep_after_reply: bool,
    /// …and without the closing chime: the exchange ended by itself.
    quiet_sleep: bool,
    /// The running turn is a player command: its success is not spoken.
    quiet_reply: bool,
    /// Make playing apps quieter during a conversation (Settings → Голос).
    duck_media: bool,
    ducking: bool,
    ducked: Vec<crate::platform::audio::Ducked>,
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
    /// A progress decode in flight, and the words it last found (with when
    /// they were first seen) for the running utterance.
    progress_job: Option<u64>,
    progress_text: Option<(u64, String, Instant)>,
    /// The utterance already answered from its stable words: the rest of it
    /// (the room's sound) is dropped when the detector closes it.
    consumed_seq: Option<u64>,
    playback_pending: bool,
    /// Speech generation whose only queued line is «Слушаю»: words spoken
    /// over it are the command it invited, not an interruption.
    ack_gen: Option<u64>,
    /// Ended without the name listener: hear nothing, close after `REST_FOR`.
    resting_since: Option<Instant>,
    /// When to say a filler if the answer hasn't started by then.
    filler_at: Option<Instant>,
    /// When the running turn started.
    turn_started: Option<Instant>,
    /// The turn the user cut off: what it still sends is not said.
    cancelled_turn: Option<u64>,
    /// The reply is held (speaker silent, queue kept) since the user began
    /// talking over it: the utterance and when. Their words decide - an
    /// interruption drops the reply, the room or an echo lets it go on.
    held: Option<(u64, Instant)>,
    /// The last lines queued for speech: what the microphone hears back as
    /// an echo through speakers.
    recent_said: VecDeque<String>,
}

impl Pipeline {
    fn emit_transcript(&self, role: &'static str, text: &str, is_final: bool) {
        let _ = self.vm.app.emit(EVT_TRANSCRIPT, TranscriptEvent { role, text: text.to_string(), is_final });
    }

    /// A line that is not the answer (filler, progress): queued like speech,
    /// but the answer still counts as unspoken.
    fn say_aside(&mut self, phrase: String) {
        self.remember_said(&phrase);
        self.queued_phrases += 1;
        let _ = self.synth_tx.send((self.speech_gen, phrase));
    }

    fn remember_said(&mut self, phrase: &str) {
        self.recent_said.push_back(phrase.to_string());
        while self.recent_said.len() > 8 {
            self.recent_said.pop_front();
        }
    }

    /// Hold the reply while the user is talking over it.
    fn hold(&mut self) {
        if self.held.is_none() {
            self.audio.set_paused(true);
            self.held = Some((self.utterance_seq, Instant::now()));
            tracing::info!("barge-in: reply held on speech onset");
        }
    }

    /// Not an interruption after all: the reply goes on where it stopped.
    fn release(&mut self, why: &str) {
        if let Some((_, since)) = self.held.take() {
            self.audio.set_paused(false);
            tracing::info!(ms = since.elapsed().as_millis() as u64, why, "barge-in: held reply resumed");
        }
    }

    fn say(&mut self, phrase: String) {
        self.remember_said(&phrase);
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
            self.duck();
        }
        self.last_activity = Instant::now();
    }

    /// Music under the user's voice is what the microphone hears instead of
    /// the command: turn playing apps down for the conversation.
    fn duck(&mut self) {
        if !self.duck_media || self.ducking || !self.ducked.is_empty() || !crate::platform::media::is_playing() {
            return;
        }
        self.ducking = true;
        let tx = self.ev_tx.clone();
        tauri::async_runtime::spawn(async move {
            let res = crate::platform::blocking(|| crate::platform::audio::duck(DUCK_TO)).await;
            let _ = tx.send(Ev::Ducked(res));
        });
    }

    /// Put the apps back as they were (unless the user moved them meanwhile).
    fn unduck(&mut self) {
        let list = std::mem::take(&mut self.ducked);
        if list.is_empty() {
            return;
        }
        let store = self.vm.app.state::<crate::state::AppState>().store.clone();
        tauri::async_runtime::spawn(async move {
            match crate::platform::blocking(move || crate::platform::audio::unduck(&list, false)).await {
                Ok(()) => {
                    let _ = store.kv_set(DUCK_KEY, "[]");
                }
                Err(e) => tracing::warn!("restoring app volumes failed: {e}"),
            }
        });
    }

    /// Back to waiting for the name: silence, cancel the running turn.
    fn sleep(&mut self) {
        self.sleep_as(false);
    }

    /// `quiet`: no closing chime. The exchange ended by itself — the answer
    /// was said, or the words were somebody else's — so JARVIS just folds;
    /// a sound after every answer, or after the room's talk, is noise.
    fn sleep_as(&mut self, quiet: bool) {
        self.hush();
        self.pending_request = None;
        self.invite_until = None;
        self.sleep_after_reply = false;
        self.ambient_line = None;
        self.utterance_seq += 1;
        if self.turn_active {
            self.cancelled_turn = Some(self.turn_seq);
            self.vm.app.state::<Arc<Agent>>().cancel();
        }
        if self.awake {
            self.awake = false;
            if !quiet {
                self.audio.play_pcm16(&AudioIo::chime_down());
            }
            self.vm.set_awake(false);
            tracing::info!("voice: waiting for the name");
        }
        self.unduck();
    }

    /// The conversation is over and nobody listens for the name: keep the
    /// devices warm, hear nothing.
    fn rest(&mut self) {
        self.rest_as(false);
    }

    fn rest_as(&mut self, quiet: bool) {
        self.sleep_as(quiet);
        self.resting_since = Some(Instant::now());
        tracing::info!("voice: resting, microphone kept open without listening");
    }

    /// The acknowledgement after the bare name is the only thing playing.
    fn ack_only(&self) -> bool {
        self.ack_gen == Some(self.speech_gen) && !self.turn_active
    }

    /// The user was asked for the command (or opened the conversation
    /// themselves): whatever they say next is it.
    fn invite_open(&self) -> bool {
        !self.turn_active && self.invite_until.is_some_and(|t| Instant::now() < t)
    }

    /// «Слушаю» (or a greeting back) after the bare name.
    fn acknowledge(&mut self, text: &str) {
        self.say(answer_call(text, self.address_vy));
        self.ack_gen = Some(self.speech_gen);
        self.invite_until = Some(Instant::now() + INVITE_FOR);
    }

    /// Drop everything queued or playing.
    fn hush(&mut self) {
        self.held = None;
        self.ack_gen = None;
        self.filler_at = None;
        self.audio.clear_playback();
        self.speech_gen += 1;
        self.synth_generation.store(self.speech_gen,std::sync::atomic::Ordering::Release);
        self.queued_phrases = 0;
        self.playback_pending = false;
    }

    fn start_turn(&mut self, text: String, heard: Heard) {
        self.turn_seq += 1;
        self.ack_gen = None;
        self.invite_until = None;
        if !heard.may_be_silent() {
            self.ambient_line = None;
        }
        self.turn_active = true;
        self.turn_heard = heard;
        self.turn_text = text.clone();
        self.splitter = PhraseSplitter::default();
        self.reply.clear();
        self.spoken_any = false;
        let words = text.rsplit("] ").next().unwrap_or(&text);
        let action = crate::agent::fast::media_action(words);
        if action.is_some() {
            self.last_media_at = Some(Instant::now());
        }
        // A player command that works is its own answer (see the agent's
        // fast path): only a failure or «что играет» is spoken.
        self.quiet_reply = action.is_some_and(|a| a != "status");
        // Words that may be the room's get no «одну секунду»: that alone
        // would be an answer to them.
        self.filler_at = (!heard.may_be_silent()).then(|| Instant::now() + FILLER_AFTER);
        self.turn_started = Some(Instant::now());
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
            let res = agent.voice_turn(text, dtx, heard).await;
            let _ = done.send(Ev::TurnDone(seq, res));
        });
    }

    /// «Стоп», «хватит»: silence, cancel, and end the voice mode — back to
    /// waiting for the name, or microphone off. When JARVIS itself is doing
    /// nothing and the music is what plays — «Джарвис, стоп» from silence, or
    /// «стоп» right after «включи музыку» — the music is what to stop.
    /// `opened_by_it`: this very phrase called JARVIS.
    fn stop_all(&mut self, opened_by_it: bool) {
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        let own = agent.is_busy() || self.turn_active || self.audio.is_playing() || self.queued_phrases > 0 || self.held.is_some();
        let after_media = self.last_media_at.is_some_and(|t| t.elapsed() < MEDIA_CHAIN);
        if !own && (opened_by_it || after_media) && crate::platform::media::is_playing() {
            tracing::info!(opened_by_it, after_media, "stop word with nothing of JARVIS's own going on: pausing the music");
            self.hush();
            self.start_turn("Пауза.".into(), Heard::Addressed);
            self.sleep_after_reply = true;
            return;
        }
        tracing::info!(busy = agent.is_busy(), "stop word");
        self.hush();
        if agent.is_busy() {
            self.cancelled_turn = Some(self.turn_seq);
            agent.cancel();
        }
        self.pending_request = None;
        if self.wake_enabled {
            self.sleep();
        } else {
            self.rest();
        }
    }

    /// «Всё, спасибо», «отбой»: drop what runs, say the goodbye (if any) and
    /// wait for the name again once it is said.
    fn dismiss(&mut self, reply: &str) {
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        self.hush();
        if self.turn_active {
            self.cancelled_turn = Some(self.turn_seq);
            agent.cancel();
        }
        self.pending_request = None;
        tracing::info!(silent = reply.is_empty(), "voice: conversation closed by the user");
        if reply.is_empty() {
            if self.wake_enabled { self.sleep() } else { self.rest() }
        } else {
            self.say(reply.to_string());
            self.sleep_after_reply = true;
            self.quiet_sleep = true;
        }
    }

    /// The name alone, or an interruption with nothing to do yet: drop what
    /// is being said and done, and listen. The words that follow are the
    /// command, not a remark.
    fn call(&mut self, text: &str) {
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        let t = Instant::now();
        self.hush();
        if self.turn_active {
            tracing::info!("running voice turn cut off by the name");
            self.cancelled_turn = Some(self.turn_seq);
            self.pending_request = None;
            agent.cancel();
        }
        self.vm.metrics.lock().last_barge_in_ms = Some(t.elapsed().as_millis() as u64);
        self.wake();
        self.acknowledge(text);
    }

    /// The user spoke up over a running task or a reply: drop it and handle
    /// the new words. A task is cancelled first and the words start the next
    /// turn when it reports back (`Ev::TurnDone`).
    fn take_over(&mut self, text: String, heard: Heard) {
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        let t = Instant::now();
        // The model is told what was cut off, so «нет, короче» or «не эту
        // папку» read as corrections of what it was doing, not new topics.
        // An exact local command («дальше», «пауза») needs no note: with one
        // it would miss the local path and wait for the model.
        let speaking = self.held.is_some() || self.audio.is_playing() || self.queued_phrases > 0;
        let text = if crate::agent::fast::command(&text).is_some() {
            text
        } else if self.turn_active {
            format!("[Пользователь перебил выполнение просьбы «{}»] {text}", short(&self.turn_text, 120))
        } else if speaking {
            format!("[Пользователь перебил твой голосовой ответ] {text}")
        } else {
            text
        };
        self.hush();
        self.last_activity = Instant::now();
        self.vm.metrics.lock().last_barge_in_ms = Some(t.elapsed().as_millis() as u64);
        if self.turn_active {
            tracing::info!(?heard, "running voice turn replaced by new words");
            self.cancelled_turn = Some(self.turn_seq);
            self.pending_request = Some((text, heard));
            agent.cancel();
        } else if agent.is_busy() {
            if !heard.may_be_silent() {
                self.say("Сейчас я выполняю задачу из чата. Скажите ещё раз, когда я закончу.".into());
            }
        } else {
            self.start_turn(text, heard);
        }
    }

    /// `heard`: how sure the words are to be for JARVIS (when unsure the
    /// model may stay silent); `opened_by_it`: this phrase called JARVIS from
    /// waiting for the name.
    fn on_request(&mut self, text: String, continues: bool, heard: Heard, opened_by_it: bool) {
        let agent = self.vm.app.state::<Arc<Agent>>().inner().clone();
        if is_stop(&text) {
            self.stop_all(opened_by_it);
            return;
        }
        // The rest of a thought is as much the user's as its start was.
        let merged = self.turn_heard.merge(heard);
        if self.turn_active && continues {
            // The user paused mid-thought and went on: one request, not two
            // answers (their speech already hushed any reply).
            tracing::info!("phrase merged into the running voice turn");
            self.pending_request = Some((format!("{} {}", self.turn_text, text), merged));
            self.cancelled_turn = Some(self.turn_seq);
            agent.cancel();
        } else if continues && !self.turn_active && !self.turn_text.is_empty() && self.turn_finished.is_some_and(|t| t.elapsed() < Duration::from_secs(6)) {
            // The answer came while the user was still talking, and their
            // speech cut it off: ask again with the whole thought.
            tracing::info!("phrase merged with the just-answered one");
            self.hush();
            let whole = format!("{} {}", self.turn_text, text);
            self.start_turn(whole, merged);
        } else if self.turn_active {
            // A new request while one runs: the newest words win at once -
            // waiting in a queue behind a long task felt like not being heard.
            tracing::info!("new request replaces the running voice turn");
            self.take_over(text, heard);
        } else if agent.is_busy() {
            if !heard.may_be_silent() {
                self.say("Сейчас я выполняю задачу из чата. Скажите ещё раз, когда я закончу.".into());
            }
        } else {
            self.start_turn(text, heard);
        }
    }

    fn derive_state(&self, vad: &Vad) -> VoiceState {
        if !self.awake {
            VoiceState::Dormant
        } else if vad.is_active() && self.consumed_seq != Some(self.utterance_seq) {
            VoiceState::Hearing
        } else if self.audio.is_playing() {
            VoiceState::Speaking
        } else if self.transcribing > 0 || (self.turn_active && self.ambient_line.is_none()) || self.queued_phrases > 0 {
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

        // Load the model while the microphone already listens. Waiting for
        // the name nobody is in a hurry (app start-up): load at background
        // priority so the desktop does not stutter.
        let engine = self.stt.clone();
        let p = model_path.clone();
        let preload = tokio::task::spawn_blocking(move || {
            if dormant { crate::platform::in_background(|| engine.ensure(&p)) } else { engine.ensure(&p) }
        });

        let (voice, rate) = (cfg.tts_voice.clone(), cfg.tts_rate);
        let speech=self.speech.clone();
        let speaker = tokio::task::spawn_blocking(move || {
            if dormant { crate::platform::in_background(|| speech.get(&voice,rate)) } else { speech.get(&voice,rate) }
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
        self.mic_failures.store(0, std::sync::atomic::Ordering::Relaxed);
        // Levels a previous run turned down and never put back (it crashed or
        // was killed): restore them by app name.
        if let Some(saved) = store.kv_get(DUCK_KEY).and_then(|j| serde_json::from_str::<Vec<crate::platform::audio::Ducked>>(&j).ok()).filter(|l| !l.is_empty()) {
            tracing::info!(apps = saved.len(), "voice: restoring app volumes left turned down");
            let st = store.clone();
            tauri::async_runtime::spawn(async move {
                if crate::platform::blocking(move || crate::platform::audio::unduck(&saved, true)).await.is_ok() {
                    let _ = st.kv_set(DUCK_KEY, "[]");
                }
            });
        }
        let initial = if dormant { VoiceState::Dormant } else { VoiceState::Listening };
        self.set_state(initial, None, Some((&audio.input_device, &audio.output_device)));
        tracing::info!(voice = %speaker.speaker.name(), dormant, "local voice session started");
        let address_vy = store.settings().persona.address == "vy";
        let ready_speaker=speaker.clone();
        tokio::task::spawn_blocking(move || {
            let _=ready_speaker.synthesize(&acknowledgement(address_vy));
            let _=ready_speaker.synthesize(&answer_call("привет", address_vy));
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
            cloud_stt: cfg.cloud_stt,
            transcribing: 0,
            turn_seq: 0,
            turn_active: false,
            turn_text: String::new(),
            last_ended: None,
            hearing_continues: false,
            turn_finished: None,
            pending_request: None,
            turn_heard: Heard::Addressed,
            ambient_line: None,
            invite_until: None,
            woke_seq: None,
            last_media_at: None,
            last_answer: String::new(),
            sleep_after_reply: false,
            quiet_sleep: false,
            quiet_reply: false,
            duck_media: cfg.duck_media,
            ducking: false,
            ducked: Vec::new(),
            splitter: PhraseSplitter::default(),
            reply: String::new(),
            spoken_any: false,
            speech_gen: 0,
            synth_generation,
            queued_phrases: 0,
            speech_end_at: None,
            first_audio_pending: false,
            shown: initial,
            utterance_seq:0,preview_job:None,hearing_since:None,progress_job:None,progress_text:None,consumed_seq:None,
            playback_pending:false,
            filler_at: None,
            turn_started: None,
            ack_gen: None,
            resting_since: None,
            cancelled_turn: None,
            held: None,
            recent_said: VecDeque::new(),
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
                    vad.endpoint_preview = pl.awake;
                    for event in vad.push(&pcm) { match event {
                        VadEvent::Started => {
                            pl.utterance_seq += 1;
                            pl.hearing_since=Some(Instant::now());
                            pl.hearing_continues = pl.last_ended.is_some_and(|e| continues_thought(e.elapsed(),pl.first_audio_pending));
                            // The conversation is not kept open by sound alone: the
                            // room's voices are not the user. Accepted requests and
                            // JARVIS's own work refresh `last_activity`.
                            // VAD is only a candidate: breaths and room noise
                            // must not destroy a spoken answer. Confirm below
                            // after ASR returns meaningful words.
                        }
                        // Resting: the microphone is open only to restart fast.
                        VadEvent::Ended(_) if !pl.awake && (!pl.wake_enabled || pl.resting_since.is_some()) => {}
                        // Already answered from its stable words: the rest is the room.
                        VadEvent::Ended(_) if pl.consumed_seq == Some(pl.utterance_seq) => {
                            pl.consumed_seq = None;
                            tracing::debug!("voice: tail of an answered phrase dropped");
                        }
                        VadEvent::Ended(utterance) => {
                            pl.transcribing += 1;
                            let ended = Instant::now();
                            pl.last_ended = Some(ended);
                            let continues = std::mem::take(&mut pl.hearing_continues);
                            let engine = self.stt.clone();
                            let path = pl.model_path.clone();
                            let tx = ev_tx.clone();
                            let dormant = !pl.awake;
                            let cloud = pl.cloud_stt && !dormant;
                            let seq = pl.utterance_seq;
                            tokio::task::spawn_blocking(move || {
                                let res = if cloud {
                                    crate::cloud::transcribe_request(&utterance, || engine.transcribe(&path, &utterance))
                                } else if dormant && utterance.len() > DORMANT_FULL_DECODE {
                                    // Only a call by name matters here: decode the
                                    // opening, and the rest only if it is one.
                                    match engine.transcribe(&path, &utterance[..DORMANT_HEAD]) {
                                        Ok(head) if super::wake::addressed(&head.text).is_none() => Ok(head),
                                        _ => engine.transcribe(&path, &utterance),
                                    }
                                } else {
                                    engine.transcribe(&path, &utterance)
                                };
                                let _ = tx.send(Ev::Transcribed(res, ended, continues, seq));
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
                        VadEvent::Progress(pcm) if pl.awake && pl.progress_job.is_none() => {
                            let seq=pl.utterance_seq;
                            pl.progress_job=Some(seq);
                            let engine=self.stt.clone();
                            let path=pl.model_path.clone();
                            let tx=ev_tx.clone();
                            tokio::task::spawn_blocking(move || { let res=engine.transcribe(&path,&pcm);let _=tx.send(Ev::Progress(seq,res)); });
                        }
                        VadEvent::Progress(_) => {}
                    } }
                    pl.refresh_state(&vad);
                }
                Some(ev) = ev_rx.recv() => match ev {
                    Ev::Progress(seq, res) => {
                        if pl.progress_job == Some(seq) { pl.progress_job = None; }
                        let Ok(t) = res else { continue };
                        if seq != pl.utterance_seq || !vad.is_active() || !stt::is_meaningful(&t.text) { continue; }
                        // Already answered from its stable words: later decodes of
                        // the same sound are the same words again. Sending them
                        // answered every such phrase twice (2026-11-18 log:
                        // «Красавчик от души, брат» → two replies 2 s apart).
                        if pl.consumed_seq == Some(seq) { continue; }
                        // Words heard over a reply that sound like an interruption:
                        // hold the reply while the rest is said. Sound alone never
                        // holds it — holding on every noise made the reply stutter
                        // (three pauses in six seconds, 2026-11-18).
                        if pl.awake && pl.held.is_none() && pl.audio.is_playing() && !pl.ack_only() {
                            let interrupting = pl.audio.headphones
                                || is_stop(&t.text)
                                || intent::interrupts_reply(&t.text)
                                || super::wake::addressed(&t.text).is_some();
                            if interrupting {
                                pl.hold();
                            }
                        }
                        let words = t.text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
                        let stable = match &pl.progress_text {
                            Some((s, w, since)) if *s == seq && *w == words => since.elapsed() >= STABLE_FOR,
                            _ => {
                                pl.progress_text = Some((seq, words, Instant::now()));
                                false
                            }
                        };
                        if stable {
                            // The words stopped changing but the detector still
                            // hears sound: the user is done, the rest is the room.
                            // Answer now; the tail of this sound is dropped.
                            tracing::info!(words = t.text.split_whitespace().count(), "voice: phrase ended by stable words");
                            pl.consumed_seq = Some(seq);
                            pl.progress_text = None;
                            pl.transcribing += 1;
                            let ended = Instant::now();
                            pl.last_ended = Some(ended);
                            let continues = std::mem::take(&mut pl.hearing_continues);
                            let _ = ev_tx.send(Ev::Transcribed(Ok(t), ended, continues, seq));
                            pl.refresh_state(&vad);
                        }
                    }
                    Ev::Ducked(res) => {
                        pl.ducking = false;
                        match res {
                            Ok(list) if !list.is_empty() => {
                                tracing::info!(apps = list.len(), "voice: playing apps turned down for the conversation");
                                if let Ok(json) = serde_json::to_string(&list) {
                                    let _ = store.kv_set(DUCK_KEY, &json);
                                }
                                pl.ducked = list;
                                // The conversation ended before the apps went down.
                                if !pl.awake {
                                    pl.unduck();
                                }
                            }
                            Ok(_) => {}
                            Err(e) => tracing::warn!("turning apps down failed: {e}"),
                        }
                    }
                    Ev::WakePreview(seq,res) => {
                        if pl.preview_job==Some(seq) { pl.preview_job=None; }
                        if !pl.awake && seq==pl.utterance_seq && res.ok().is_some_and(|t| super::wake::addressed(&t.text).is_some()) {
                            let ms=pl.hearing_since.map(|t|t.elapsed().as_millis() as u64).unwrap_or(0);
                            tracing::info!(ms,"voice: early name detected");
                            self.metrics.lock().last_wake_ms=Some(ms);
                            pl.woke_seq = Some(seq);
                            pl.wake();
                            // Heard: a short cue now, the answer after the command.
                            pl.audio.play_pcm16(&AudioIo::chime());
                            pl.refresh_state(&vad);
                        }
                    }
                    Ev::Transcribed(res, ended, continues, seq) => {
                        pl.transcribing = pl.transcribing.saturating_sub(1);
                        match res {
                            Ok(t) => {
                                self.metrics.lock().last_stt_ms = Some(t.ms);
                                let words = t.text.split_whitespace().count();
                                tracing::info!(ms = t.ms, words, "speech recognised");
                                let meaningful = stt::is_meaningful(&t.text);
                                // Words over «Слушаю» (or after a click or the shortcut)
                                // are the command itself.
                                let invited = pl.invite_open();
                                // This utterance's opening woke JARVIS: it is addressed,
                                // whatever the final decode made of the name.
                                let woke_here = pl.woke_seq == Some(seq);
                                let reply_busy = !invited && (pl.audio.is_playing() || pl.queued_phrases > 0);
                                if invited && meaningful {
                                    pl.hush();
                                }
                                // Awake with the name listener on: whose words are these?
                                // While JARVIS works or speaks only a stop word, the name
                                // or the rest of a thought count — not the film or the
                                // person next to the user (see `intent`).
                                let mut t = t;
                                // Through speakers a loud line of the reply can open the
                                // detector by itself: that is JARVIS hearing JARVIS.
                                if pl.held.is_some() && !pl.audio.headphones && is_echo(&t.text, &pl.recent_said) {
                                    tracing::info!(words, "barge-in: own voice heard back, not an interruption");
                                    pl.release("echo");
                                    pl.refresh_state(&vad);
                                    continue;
                                }
                                // The same words again within seconds of starting a turn
                                // on them: a second decode of one phrase, not a request.
                                if pl.awake && meaningful && !pl.turn_text.is_empty()
                                    && pl.turn_started.is_some_and(|s| s.elapsed() < REPEAT_WINDOW)
                                    && same_words(&t.text, &pl.turn_text)
                                {
                                    tracing::info!(words, "voice: repeated decode of the running request dropped");
                                    pl.refresh_state(&vad);
                                    continue;
                                }
                                // The cue heard the name, the decode lost it («Джас.»).
                                // Without even a mangled name the cue was wrong: the
                                // phrase is judged like any unnamed one, and if it is not
                                // for JARVIS the conversation it opened closes at once.
                                let mut false_cue = false;
                                if pl.awake && woke_here && meaningful && super::wake::addressed(&t.text).is_none() && super::wake::from_name(&t.text, 4).is_none() {
                                    match after_lost_name(&t.text) {
                                        LostName::Call => {
                                            pl.emit_transcript("user", &t.text, true);
                                            pl.call(&t.text);
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        LostName::Command(named) => t.text = named,
                                        LostName::NotACall => false_cue = true,
                                    }
                                }
                                if pl.awake && pl.wake_enabled && meaningful {
                                    let ctx = intent::Context {
                                        busy: pl.turn_active || reply_busy,
                                        speaking: reply_busy,
                                        turn_active: pl.turn_active,
                                        continues,
                                        turn_age: pl.turn_started.map_or(Duration::ZERO, |s| s.elapsed()),
                                        headphones: pl.audio.headphones,
                                        media_playing: crate::platform::media::is_playing(),
                                        media_chain: pl.last_media_at.is_some_and(|t| t.elapsed() < MEDIA_CHAIN),
                                        asked: pl.last_answer.trim_end().ends_with('?') && pl.turn_finished.is_some_and(|t| t.elapsed() < SLEEP_AFTER),
                                        invited,
                                    };
                                    match intent::classify(&t.text, &ctx) {
                                        Intent::Ignore => {
                                            let busy = pl.turn_active || reply_busy;
                                            tracing::info!(words, busy, "voice: words not for JARVIS ignored");
                                            // Others are talking (a whole sentence, not a cough), or
                                            // the early cue misheard: stop listening without the name
                                            // until called. Not in the middle of a «дальше… дальше…»
                                            // run, where the song itself is what the mic hears.
                                            let chain = pl.last_media_at.is_some_and(|t| t.elapsed() < MEDIA_CHAIN);
                                            if !busy && !invited && (false_cue || (words >= 3 && !chain)) {
                                                tracing::info!("voice: the room is talking, back to waiting for the name");
                                                pl.sleep_as(true);
                                            }
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        Intent::Stop => {
                                            pl.emit_transcript("user", &t.text, true);
                                            pl.stop_all(woke_here);
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        Intent::Dismiss(reply) => {
                                            pl.emit_transcript("user", &t.text, true);
                                            pl.dismiss(reply);
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        Intent::Ambient { text, cut } => {
                                            tracing::info!(words, cut, "voice: unnamed words for the model to judge");
                                            // Shown only once the model answers them.
                                            pl.ambient_line = Some(text.clone());
                                            pl.speech_end_at = Some(ended);
                                            pl.first_audio_pending = true;
                                            if cut { pl.take_over(text, Heard::Unnamed) } else { pl.on_request(text, continues, Heard::Unnamed, false) }
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        Intent::Unsure(text) => {
                                            tracing::info!(words, "voice: named words that read like talk, for the model to judge");
                                            pl.ambient_line = Some(text.clone());
                                            pl.speech_end_at = Some(ended);
                                            pl.first_audio_pending = true;
                                            pl.on_request(text, continues, Heard::Unsure, woke_here);
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        Intent::Call => {
                                            pl.emit_transcript("user", &t.text, true);
                                            pl.call(&t.text);
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        Intent::Replace(text) => {
                                            tracing::info!(words, "voice: user spoke over the running turn");
                                            pl.emit_transcript("user", &text, true);
                                            pl.speech_end_at = Some(ended);
                                            pl.first_audio_pending = true;
                                            pl.take_over(text, Heard::Addressed);
                                            pl.refresh_state(&vad);
                                            continue;
                                        }
                                        // The words to use, with any chatter before the name cut off.
                                        Intent::Request(text) => t.text = text,
                                    }
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
                                    pl.acknowledge(&t.text);
                                } else if !pl.awake {
                                    // Waiting for the name: phrases not addressed to JARVIS
                                    // are dropped here — not shown, logged or sent.
                                    match super::wake::addressed(&t.text) {
                                        Some(a) => {
                                            tracing::info!(only_name = a.only_name, "voice: called by name");
                                            // Named: the same reading as when awake and idle.
                                            let intent = intent::classify(&t.text, &intent::Context::default());
                                            if intent != Intent::Ignore {
                                                pl.wake();
                                            }
                                            // Talk with the name in it is shown only once the
                                            // model answers it.
                                            if !matches!(intent, Intent::Ignore | Intent::Unsure(_)) {
                                                pl.emit_transcript("user", &t.text, true);
                                            }
                                            match intent {
                                                Intent::Call => pl.acknowledge(&t.text),
                                                Intent::Stop => pl.stop_all(true),
                                                Intent::Dismiss(reply) => pl.dismiss(reply),
                                                Intent::Ignore => tracing::info!("voice: named, but for another assistant"),
                                                Intent::Unsure(text) => {
                                                    tracing::info!(words, "voice: named words that read like talk, for the model to judge");
                                                    pl.ambient_line = Some(text.clone());
                                                    pl.speech_end_at = Some(ended);
                                                    pl.first_audio_pending = true;
                                                    pl.on_request(text, false, Heard::Unsure, true);
                                                }
                                                Intent::Request(text) | Intent::Replace(text) | Intent::Ambient { text, .. } => {
                                                    pl.speech_end_at = Some(ended);
                                                    pl.first_audio_pending = true;
                                                    pl.on_request(text, false, Heard::Addressed, true);
                                                }
                                            }
                                        }
                                        // Player commands count from anyone, without the name (the
                                        // user's choice, 2026-11-21): exact player words only,
                                        // never over JARVIS's own voice or as an echo of it, never
                                        // into a chat task he is doing. A bare «стоп» pauses the
                                        // music when JARVIS himself has nothing going on.
                                        None if !reply_busy
                                            && intent::player_command(&t.text).is_some()
                                            && (pl.audio.headphones || !is_echo(&t.text, &pl.recent_said))
                                            && !pl.vm.app.state::<Arc<Agent>>().is_busy() =>
                                        {
                                            let command = intent::player_command(&t.text).unwrap_or_default();
                                            tracing::info!(words, "voice: player command without the name");
                                            pl.speech_end_at = Some(ended);
                                            pl.first_audio_pending = true;
                                            pl.on_request(command, false, Heard::Addressed, true);
                                        }
                                        // The text stays in the local log only: it shows how
                                        // the recogniser hears the name when a call is missed.
                                        None => tracing::debug!(words, text = %t.text.chars().take(80).collect::<String>(), "voice: not addressed"),
                                    }
                                } else {
                                    pl.emit_transcript("user", &t.text, true);
                                    pl.speech_end_at = Some(ended);
                                    pl.first_audio_pending = true;
                                    pl.on_request(t.text, continues, Heard::Addressed, woke_here);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("speech recognition failed: {e}");
                                break Err(e);
                            }
                        }
                        pl.refresh_state(&vad);
                    }
                    Ev::Delta(seq, d) if seq == pl.turn_seq && pl.cancelled_turn != Some(seq) => {
                        pl.filler_at = None;
                        if let Some(line) = pl.ambient_line.take() {
                            pl.emit_transcript("user", &line, true);
                        }
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
                        let heard = std::mem::take(&mut pl.turn_heard);
                        pl.ambient_line = None;
                        match res {
                            // Cut off by the user just as it finished: say nothing of it.
                            Ok(_) if pl.cancelled_turn == Some(seq) => {}
                            // The model judged the words somebody else's: nothing was
                            // said, and JARVIS folds without a sound.
                            Ok(final_text) if heard.may_be_silent() && final_text.trim() == crate::agent::SILENT => {
                                tracing::info!(?heard, "voice: the model judged the words not meant for JARVIS");
                                pl.turn_text.clear();
                                pl.last_answer.clear();
                                if pl.pending_request.is_none() {
                                    pl.sleep_as(true);
                                }
                            }
                            Ok(final_text) => {
                                if let Some(rest) = pl.splitter.finish() {
                                    pl.say(rest);
                                } else if !pl.spoken_any && !final_text.trim().is_empty() && !pl.quiet_reply {
                                    pl.say(super::phrases::speakable(&final_text));
                                }
                                let shown = if pl.reply.trim().is_empty() { final_text } else { pl.reply.clone() };
                                pl.emit_transcript("assistant", shown.trim(), true);
                                pl.last_answer = shown.trim().to_string();
                                // The request is done: stop listening once the answer is
                                // said. Staying open for follow-ups made JARVIS answer the
                                // room after every command. Only a question back keeps
                                // the microphone open for the reply.
                                if !asks_back(&pl.last_answer) {
                                    pl.sleep_after_reply = true;
                                    pl.quiet_sleep = true;
                                }
                            }
                            Err(e) => {
                                let msg = e.to_string();
                                // Words that may have been the room's get no error aloud either.
                                if !heard.may_be_silent() && pl.pending_request.is_none() && !msg.contains("отмен") && !msg.to_lowercase().contains("cancel") {
                                    pl.audio.play_pcm16(&AudioIo::chime_error());
                                    pl.say(format!("Не получилось: {}", super::phrases::speakable(&msg)));
                                }
                                if pl.cancelled_turn != Some(seq) {
                                    pl.sleep_after_reply = true;
                                    pl.quiet_sleep = true;
                                }
                            }
                        }
                        if let Some((next, next_heard)) = pl.pending_request.take() {
                            pl.hush();
                            if next_heard.may_be_silent() {
                                pl.ambient_line = Some(next.rsplit("] ").next().unwrap_or(&next).to_string());
                            }
                            pl.sleep_after_reply = false;
                            pl.start_turn(next, next_heard);
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
                Some(_title) = confirm_rx.recv() => {
                    // The command is on the island with its buttons; reading it
                    // aloud only got in the way. A short cue says «look».
                    if pl.turn_active {
                        pl.audio.play_pcm16(&AudioIo::chime_confirm());
                        pl.refresh_state(&vad);
                    }
                }
                Some(c) = ctl_rx.recv() => {
                    match c {
                        Ctl::Wake => {
                            pl.wake();
                            pl.invite_until = Some(Instant::now() + INVITE_FOR);
                        }
                        Ctl::Sleep => pl.sleep(),
                        Ctl::Rest => pl.rest(),
                        Ctl::Hush => pl.hush(),
                        Ctl::Say(line) => pl.say(line),
                    }
                    pl.refresh_state(&vad);
                }
                _ = level_tick.tick() => {
                    // Held, and nothing is being said or recognised any more:
                    // the words (if any) did not interrupt - go on. A long
                    // stretch of sound (a film) cannot hold the reply forever.
                    if let Some((_, since)) = pl.held {
                        let quiet = !vad.is_active() && pl.transcribing == 0 && since.elapsed() >= HOLD_MIN;
                        if quiet || since.elapsed() >= HOLD_MAX {
                            pl.release(if quiet { "no interruption" } else { "held too long" });
                            pl.refresh_state(&vad);
                        }
                    }
                    if pl.turn_active && pl.filler_at.is_some_and(|t| Instant::now() >= t) {
                        pl.filler_at = None;
                        if pl.queued_phrases == 0 && !pl.audio.is_playing() {
                            pl.say_aside(FILLERS[pl.turn_seq as usize % FILLERS.len()].to_string());
                        }
                    }
                    if pl.playback_pending && !pl.turn_active && pl.queued_phrases == 0 && !pl.audio.is_playing() {
                        tracing::info!(turn=pl.turn_seq,"voice playback drained");
                        pl.playback_pending = false;
                    }
                    // Not while the user is still talking: words over the end of
                    // the answer are theirs to finish.
                    if pl.sleep_after_reply && !pl.turn_active && pl.queued_phrases == 0 && !pl.audio.is_playing() && !vad.is_active() && pl.transcribing == 0 {
                        pl.sleep_after_reply = false;
                        let quiet = std::mem::take(&mut pl.quiet_sleep);
                        if pl.wake_enabled { pl.sleep_as(quiet) } else { pl.rest_as(quiet) }
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
        pl.unduck();
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
    fn a_name_the_final_decode_mangled_is_still_a_call_and_a_wrong_cue_is_none() {
        assert_eq!(after_lost_name("Джас."), LostName::Call);
        assert_eq!(after_lost_name("Джас. Включи музыку."), LostName::Command("Джарвис, Включи музыку.".into()));
        assert_eq!(after_lost_name("Жаз, дальше"), LostName::Command("Джарвис, дальше".into()));
        // The cue misheard «Дар…» in «Дарья»: no call.
        assert_eq!(after_lost_name("Дарья, привет."), LostName::NotACall);
        assert_eq!(after_lost_name("Включи музыку."), LostName::NotACall);
        assert_eq!(after_lost_name(""), LostName::NotACall);
    }

    #[test]
    fn a_second_decode_of_the_running_request_is_recognised() {
        assert!(same_words("Красавчик, от души, брат!", "Красавчик от души брат."));
        assert!(same_words("закрой тикток", "[Пользователь перебил твой голосовой ответ] Закрой ТикТок."));
        assert!(!same_words("закрой тикток", "открой тикток"));
        assert!(!same_words("тикток", "[Пользователь перебил] открой тикток и ютуб"));
        assert!(!same_words("", "что угодно"));
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
    fn only_a_question_back_keeps_the_microphone_open_after_an_answer() {
        for a in ["Включил.", "Сделал потише — тридцать процентов.", "Играет «Про любовь» — Земфира", "", "Готово!"] {
            assert!(!asks_back(a), "{a}");
        }
        for a in ["Какую папку открыть?", "Отправить Маше?  ", "Ты про «Ведьму»?»", "Ты про «Ведьму?»"] {
            assert!(asks_back(a), "{a}");
        }
    }

    #[test]
    fn a_new_command_after_a_spoken_answer_is_not_replayed_with_the_previous_action() {
        assert!(continues_thought(Duration::from_millis(500),true));
        assert!(!continues_thought(Duration::from_millis(500),false));
        assert!(!continues_thought(Duration::from_secs(3),true));
    }
}
