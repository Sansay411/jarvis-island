//! Live board games with JARVIS: Chess, Russian Checkers and Battleship.
//!
//! Coordinates the `games` window, persists the head-to-head rivalry scoreboard
//! in SQLite `kv`, keeps a verified live snapshot of the board for the main
//! agent's turn context and `game_play` tool, and runs unscripted streaming LLM
//! commentary with live face emotes and neural TTS readout.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;

use crate::ai::types::{ChatMessage, Role, StreamDelta};
use crate::error::{AppError, AppResult};
use crate::store::Store;

pub const GAMES_LABEL: &str = "games";
pub const EVT_REMOTE_CMD: &str = "games:remote-cmd";
pub const EVT_COMMENT: &str = "games:comment";
pub const EVT_STATS: &str = "games:stats";
const KV_STATS: &str = "games.stats";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameStatsEntry {
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    /// Positive = player's win streak; negative = JARVIS's win streak.
    pub streak: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllGameStats {
    pub chess: GameStatsEntry,
    pub checkers: GameStatsEntry,
    pub battleship: GameStatsEntry,
}

impl AllGameStats {
    pub fn entry_mut(&mut self, game: &str) -> Option<&mut GameStatsEntry> {
        match game {
            "chess" => Some(&mut self.chess),
            "checkers" => Some(&mut self.checkers),
            "battleship" => Some(&mut self.battleship),
            _ => None,
        }
    }

    pub fn entry(&self, game: &str) -> Option<&GameStatsEntry> {
        match game {
            "chess" => Some(&self.chess),
            "checkers" => Some(&self.checkers),
            "battleship" => Some(&self.battleship),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveGameSnapshot {
    pub open: bool,
    pub game: Option<String>,
    pub phase: String,
    pub difficulty: String,
    pub mood: String,
    pub voice_enabled: bool,
    pub turn: String,
    pub move_count: u32,
    pub score_summary: String,
    pub board_summary: String,
    pub last_moves: Vec<String>,
    pub winner: Option<String>,
    pub status_line: String,
}

impl Default for ActiveGameSnapshot {
    fn default() -> Self {
        Self {
            open: false,
            game: None,
            phase: "lobby".into(),
            difficulty: "medium".into(),
            mood: "auto".into(),
            voice_enabled: true,
            turn: "none".into(),
            move_count: 0,
            score_summary: String::new(),
            board_summary: String::new(),
            last_moves: Vec::new(),
            winner: None,
            status_line: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGameCommand {
    pub request_id: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mood: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_str: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGameReply {
    pub request_id: String,
    pub ok: bool,
    pub summary: String,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentRequest {
    pub game: String,
    pub difficulty: String,
    pub mood: String,
    pub trigger: String,
    pub move_count: u32,
    #[serde(default)]
    pub player_move: Option<String>,
    #[serde(default)]
    pub jarvis_move: Option<String>,
    #[serde(default)]
    pub tactical_notes: Vec<String>,
    pub score_summary: String,
    pub board_summary: String,
    #[serde(default)]
    pub recent_jarvis_lines: Vec<String>,
    #[serde(default)]
    pub player_say: Option<String>,
    #[serde(default)]
    pub speak: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentStreamEvent {
    pub req_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mood_switch: Option<String>,
}

struct Inner {
    snapshot: ActiveGameSnapshot,
    frontend_ready: bool,
    pending_boot_cmd: Option<RemoteGameCommand>,
    waiters: HashMap<String, oneshot::Sender<RemoteGameReply>>,
    comment_cancel: Option<CancellationToken>,
}

pub struct GamesHub {
    app: Option<AppHandle>,
    store: Arc<Store>,
    inner: Mutex<Inner>,
    ready_notify: Notify,
    comment_seq: AtomicU64,
}

pub fn game_title_ru(game: &str) -> &'static str {
    match game {
        "chess" => "Шахматы",
        "checkers" => "Русские шашки",
        "battleship" => "Морской бой",
        _ => "Игры",
    }
}

pub fn mood_title_ru(mood: &str) -> &'static str {
    match mood {
        "joker" => "шутник и душа компании",
        "business" => "деловой клубный игрок",
        "rival" => "азартный соперник",
        _ => "свой характер Джарвиса",
    }
}

pub fn difficulty_ru(diff: &str) -> &'static str {
    match diff {
        "easy" => "лёгкий",
        "hard" => "мастер",
        _ => "соперник",
    }
}

const VALID_EMOTES: &[&str] = &[
    "wink", "heart", "laugh", "surprised", "shake", "boop", "star",
    "sad", "determined", "squint", "confused", "dizzy", "yawn", "wake",
];

const VALID_MOODS: &[&str] = &["auto", "joker", "business", "rival"];

impl GamesHub {
    pub fn new(app: Option<AppHandle>, store: Arc<Store>) -> Arc<Self> {
        Arc::new(Self {
            app,
            store,
            inner: Mutex::new(Inner {
                snapshot: ActiveGameSnapshot::default(),
                frontend_ready: false,
                pending_boot_cmd: None,
                waiters: HashMap::new(),
                comment_cancel: None,
            }),
            ready_notify: Notify::new(),
            comment_seq: AtomicU64::new(0),
        })
    }

    pub fn stats(&self) -> AllGameStats {
        self.store
            .kv_get(KV_STATS)
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn record_finish(&self, game: &str, winner: &str) -> AppResult<AllGameStats> {
        let mut all = self.stats();
        let wins = {
            let Some(entry) = all.entry_mut(game) else {
                return Err(AppError::invalid(format!("Неизвестная игра: {game}")));
            };
            match winner {
                "player" => {
                    entry.wins += 1;
                    entry.streak = if entry.streak >= 0 { entry.streak + 1 } else { 1 };
                }
                "jarvis" => {
                    entry.losses += 1;
                    entry.streak = if entry.streak <= 0 { entry.streak - 1 } else { -1 };
                }
                _ => {
                    entry.draws += 1;
                    entry.streak = 0;
                }
            }
            entry.wins
        };
        let json = serde_json::to_string(&all)?;
        self.store.kv_set(KV_STATS, &json)?;
        if let Some(app) = &self.app {
            let _ = app.emit(EVT_STATS, &all);
            if let Some(mentor) = app.try_state::<Arc<crate::mentor::Mentor>>() {
                let xp = match winner {
                    "player" => 35,
                    "draw" => 22,
                    _ => 15,
                };
                let _ = mentor.complete("game", "mind", game_title_ru(game), 4.0, wins, xp, false);
            }
        }
        Ok(all)
    }

    pub fn snapshot(&self) -> ActiveGameSnapshot {
        self.inner.lock().snapshot.clone()
    }

    pub fn sync_snapshot(&self, mut snap: ActiveGameSnapshot) {
        if let Some(app) = &self.app {
            snap.open = app.get_webview_window(GAMES_LABEL).is_some();
        }
        self.inner.lock().snapshot = snap;
    }

    pub fn on_window_closed(&self) {
        let mut i = self.inner.lock();
        i.frontend_ready = false;
        i.snapshot.open = false;
        if let Some(c) = i.comment_cancel.take() {
            c.cancel();
        }
    }

    /// Called by `GamesApp` on mount; returns any initial command queued when
    /// opening the window.
    pub fn mark_ready(&self) -> Option<RemoteGameCommand> {
        let cmd = {
            let mut i = self.inner.lock();
            i.frontend_ready = true;
            i.snapshot.open = true;
            i.pending_boot_cmd.take()
        };
        self.ready_notify.notify_waiters();
        cmd
    }

    pub fn open_window(&self, boot_cmd: Option<RemoteGameCommand>) -> AppResult<bool> {
        let Some(app) = &self.app else {
            return Ok(false);
        };
        if let Some(w) = app.get_webview_window(GAMES_LABEL) {
            let _ = w.unminimize();
            w.show()?;
            w.set_focus()?;
            if let Some(cmd) = boot_cmd {
                if self.inner.lock().frontend_ready {
                    let _ = app.emit_to(GAMES_LABEL, EVT_REMOTE_CMD, &cmd);
                } else {
                    self.inner.lock().pending_boot_cmd = Some(cmd);
                }
            }
            return Ok(false);
        }

        {
            let mut i = self.inner.lock();
            i.frontend_ready = false;
            i.pending_boot_cmd = boot_cmd;
            i.snapshot.open = true;
        }
        WebviewWindowBuilder::new(app, GAMES_LABEL, WebviewUrl::App("index.html".into()))
            .title("Джарвис — игры")
            .inner_size(1160.0, 780.0)
            .min_inner_size(940.0, 640.0)
            .center()
            .theme(Some(tauri::Theme::Dark))
            .background_color(tauri::webview::Color(8, 8, 10, 255))
            .decorations(false)
            .shadow(true)
            .build()?;
        Ok(true)
    }

    /// Execute an interactive board command from the main voice/chat agent and
    /// wait for the verified board outcome from `GamesApp`.
    pub async fn execute_remote(&self, mut cmd: RemoteGameCommand) -> AppResult<RemoteGameReply> {
        let Some(app) = &self.app else {
            return Err(AppError::internal("Окно игр недоступно в тестовом окружении"));
        };
        if cmd.request_id.is_empty() {
            cmd.request_id = uuid::Uuid::new_v4().to_string();
        }
        let req_id = cmd.request_id.clone();
        let (tx, rx) = oneshot::channel();
        self.inner.lock().waiters.insert(req_id.clone(), tx);

        // Ensure window is open and mounted
        let created = self.open_window(None)?;
        if created || !self.inner.lock().frontend_ready {
            let _ = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if self.inner.lock().frontend_ready {
                        break;
                    }
                    self.ready_notify.notified().await;
                }
            })
            .await;
        }

        let _ = app.emit_to(GAMES_LABEL, EVT_REMOTE_CMD, &cmd);

        match tokio::time::timeout(Duration::from_secs(7), rx).await {
            Ok(Ok(reply)) => Ok(reply),
            _ => {
                self.inner.lock().waiters.remove(&req_id);
                let snap = self.snapshot();
                Ok(RemoteGameReply {
                    request_id: req_id,
                    ok: true,
                    summary: format!(
                        "Открыто игровое окно: {}",
                        snap.game.as_deref().map(game_title_ru).unwrap_or("выбор игры")
                    ),
                    data: serde_json::to_value(&snap).unwrap_or_else(|_| json!({})),
                })
            }
        }
    }

    pub fn resolve_remote(&self, reply: RemoteGameReply) {
        if let Some(tx) = self.inner.lock().waiters.remove(&reply.request_id) {
            let _ = tx.send(reply);
        }
    }

    /// Concise context line injected into the main agent's `[Контекст запроса]`
    /// whenever the games window is open, so JARVIS always knows the board!
    pub fn context_line(&self) -> Option<String> {
        let snap = self.inner.lock().snapshot.clone();
        if !snap.open {
            return None;
        }
        let stats = self.stats();
        let Some(game) = snap.game.as_deref() else {
            return Some(format!(
                "- Active games window: открыто лобби «Игры с Джарвисом» (шахматы {}:{}, шашки {}:{}, морской бой {}:{}). Используй `game_play`, чтобы запустить партию или сделать ход.\n",
                stats.chess.wins, stats.chess.losses,
                stats.checkers.wins, stats.checkers.losses,
                stats.battleship.wins, stats.battleship.losses,
            ));
        };
        let st = stats.entry(game).cloned().unwrap_or_default();
        let moves = if snap.last_moves.is_empty() {
            "ходов ещё нет".to_string()
        } else {
            snap.last_moves.join(", ")
        };
        Some(format!(
            "- Active game with user: {} (фаза: {}, сложность: {}, твоя манера за столом: {}, ход №{}, сейчас ход: {}, общий счёт побед игрок/Джарвис: {}/{}, ничьих: {}). Баланс: {}. Последние ходы: {}. Доска: {}. Для хода, реванша, смены манеры или подсказки используй `game_play`.\n",
            game_title_ru(game),
            snap.phase,
            difficulty_ru(&snap.difficulty),
            mood_title_ru(&snap.mood),
            snap.move_count,
            snap.turn,
            st.wins,
            st.losses,
            st.draws,
            snap.score_summary,
            moves,
            snap.board_summary,
        ))
    }

    /// Generate a live, unscripted streaming remark from JARVIS for the current
    /// board moment or table chat message, firing face emotes and optional TTS.
    pub fn spawn_comment(self: &Arc<Self>, req: CommentRequest) -> u64 {
        let req_id = self.comment_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = CancellationToken::new();
        {
            let mut i = self.inner.lock();
            if let Some(prev) = i.comment_cancel.replace(cancel.clone()) {
                prev.cancel();
            }
        }
        let hub = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            hub.run_comment(req_id, req, cancel).await;
        });
        req_id
    }

    async fn run_comment(&self, req_id: u64, req: CommentRequest, cancel: CancellationToken) {
        let Some(app) = &self.app else { return };
        let settings = self.store.settings();
        let stats = self.stats().entry(&req.game).cloned().unwrap_or_default();
        let memories = self.store.memories().unwrap_or_default();
        let humor_lessons: Vec<String> = if settings.agent.self_learning {
            crate::learn::cached()
                .into_iter()
                .filter(|l| l.kind == "humor" || l.kind == "preference")
                .take(6)
                .map(|l| l.value)
                .collect()
        } else {
            Vec::new()
        };

        let system = build_comment_system_prompt(&settings, &req.mood, &memories, &humor_lessons);
        let user = build_comment_user_prompt(&req, &stats);

        let Ok(client) = crate::ai::client_from_settings(&settings, &self.store) else {
            let (emote, fallback) = dynamic_fallback(&req);
            self.emit_complete_fallback(app, req_id, emote, fallback, req.speak);
            return;
        };

        let msgs = [
            ChatMessage::text(Role::System, system),
            ChatMessage::text(Role::User, user),
        ];

        let parser = Arc::new(Mutex::new(TagStreamParser::default()));
        let app_stream = app.clone();
        let parser_stream = Arc::clone(&parser);

        let res = tokio::time::timeout(
            Duration::from_secs(18),
            client.complete(&msgs, &[], &cancel, Some(220), move |delta| {
                if let StreamDelta::Text(chunk) = delta {
                    let mut p = parser_stream.lock();
                    let (emote, mood_sw, text_out) = p.push(&chunk);
                    if let Some(ref em) = emote {
                        let _ = app_stream.emit_to(crate::platform::island::LABEL, "island:emote", em.as_str());
                    }
                    if emote.is_some() || mood_sw.is_some() || !text_out.is_empty() {
                        let _ = app_stream.emit_to(
                            GAMES_LABEL,
                            EVT_COMMENT,
                            CommentStreamEvent {
                                req_id,
                                delta: (!text_out.is_empty()).then_some(text_out),
                                done: None,
                                full_text: None,
                                emote,
                                mood_switch: mood_sw,
                            },
                        );
                    }
                }
            }),
        )
        .await;

        if cancel.is_cancelled() {
            return;
        }

        match res {
            Ok(Ok(_)) => {
                let (emote, mood_sw, full_text) = parser.lock().finish();
                if let Some(ref em) = emote {
                    let _ = app.emit_to(crate::platform::island::LABEL, "island:emote", em.as_str());
                }
                let final_clean = clean_spoken_line(&full_text);
                if final_clean.is_empty() {
                    let (em_fb, text_fb) = dynamic_fallback(&req);
                    self.emit_complete_fallback(app, req_id, em_fb, text_fb, req.speak);
                    return;
                }
                let _ = app.emit_to(
                    GAMES_LABEL,
                    EVT_COMMENT,
                    CommentStreamEvent {
                        req_id,
                        delta: None,
                        done: Some(true),
                        full_text: Some(final_clean.clone()),
                        emote,
                        mood_switch: mood_sw,
                    },
                );
                if req.speak {
                    speak_if_idle(app, &final_clean);
                }
            }
            _ => {
                let (em_fb, text_fb) = dynamic_fallback(&req);
                self.emit_complete_fallback(app, req_id, em_fb, text_fb, req.speak);
            }
        }
    }

    fn emit_complete_fallback(
        &self,
        app: &AppHandle,
        req_id: u64,
        emote: &'static str,
        text: String,
        speak: bool,
    ) {
        let _ = app.emit_to(crate::platform::island::LABEL, "island:emote", emote);
        let _ = app.emit_to(
            GAMES_LABEL,
            EVT_COMMENT,
            CommentStreamEvent {
                req_id,
                delta: Some(text.clone()),
                done: Some(true),
                full_text: Some(text.clone()),
                emote: Some(emote.into()),
                mood_switch: None,
            },
        );
        if speak {
            speak_if_idle(app, &text);
        }
    }
}

fn speak_if_idle(app: &AppHandle, text: &str) {
    let Some(vm) = app.try_state::<Arc<crate::voice::VoiceManager>>() else { return };
    // Don't interrupt active user speech capture on the mic
    if matches!(vm.state(), crate::voice::VoiceState::Hearing | crate::voice::VoiceState::Thinking) {
        return;
    }
    let spoken = strip_for_tts(text);
    if !spoken.is_empty() {
        let _ = vm.speak(spoken, None, None, true);
    }
}

fn strip_for_tts(s: &str) -> String {
    s.replace('*', "")
        .replace('`', "")
        .replace('#', "")
        .trim()
        .to_string()
}

fn clean_spoken_line(s: &str) -> String {
    let trimmed = s.trim().trim_matches('"').trim();
    trimmed.to_string()
}

/// Parses leading bracket tags like `[emote:laugh][mood:business] Ну что ж...`
/// during streaming without leaking partial brackets into the UI bubble.
#[derive(Default)]
pub struct TagStreamParser {
    buf: String,
    in_body: bool,
    emitted_body: String,
    final_emote: Option<String>,
    final_mood: Option<String>,
}

impl TagStreamParser {
    pub fn push(&mut self, chunk: &str) -> (Option<String>, Option<String>, String) {
        if self.in_body {
            self.emitted_body.push_str(chunk);
            return (None, None, chunk.to_string());
        }
        self.buf.push_str(chunk);
        let mut new_emote = None;
        let mut new_mood = None;

        loop {
            let trimmed = self.buf.trim_start().to_string();
            if trimmed.is_empty() {
                self.buf.clear();
                return (new_emote, new_mood, String::new());
            }
            if !trimmed.starts_with('[') {
                self.in_body = true;
                self.emitted_body.push_str(&trimmed);
                self.buf.clear();
                return (new_emote, new_mood, trimmed);
            }
            if let Some(end) = trimmed.find(']') {
                let tag = &trimmed[1..end];
                if let Some(val) = tag.strip_prefix("emote:") {
                    let v = val.trim().to_lowercase();
                    if VALID_EMOTES.contains(&v.as_str()) {
                        self.final_emote = Some(v.clone());
                        new_emote = Some(v);
                    }
                } else if let Some(val) = tag.strip_prefix("mood:") {
                    let v = val.trim().to_lowercase();
                    if VALID_MOODS.contains(&v.as_str()) {
                        self.final_mood = Some(v.clone());
                        new_mood = Some(v);
                    }
                }
                self.buf = trimmed[end + 1..].to_string();
                if self.buf.len() > 80 {
                    self.in_body = true;
                    let rest = self.buf.trim_start().to_string();
                    self.emitted_body.push_str(&rest);
                    self.buf.clear();
                    return (new_emote, new_mood, rest);
                }
            } else {
                // Still waiting for closing `]`
                if trimmed.len() > 42 {
                    // Malformed bracket, flush as body
                    self.in_body = true;
                    self.emitted_body.push_str(&trimmed);
                    self.buf.clear();
                    return (new_emote, new_mood, trimmed);
                }
                return (new_emote, new_mood, String::new());
            }
        }
    }

    pub fn finish(&mut self) -> (Option<String>, Option<String>, String) {
        if !self.in_body && !self.buf.trim().is_empty() {
            let leftover = self.buf.trim().to_string();
            self.emitted_body.push_str(&leftover);
            self.buf.clear();
            self.in_body = true;
        }
        (self.final_emote.take(), self.final_mood.take(), self.emitted_body.clone())
    }
}

fn build_comment_system_prompt(
    settings: &crate::store::settings::Settings,
    mood: &str,
    memories: &[crate::store::Memory],
    humor_lessons: &[String],
) -> String {
    let p = &settings.persona;
    let addr = if p.address == "vy" {
        "Обращайся к собеседнику строго на «вы»."
    } else {
        "Обращайся к собеседнику на «ты» — естественно, как к хорошему другу и партнёру."
    };
    let name_line = if p.user_name.trim().is_empty() {
        String::new()
    } else {
        format!("Имя игрока: {} (используй изредка и к месту, не в каждой фразе). ", p.user_name.trim())
    };

    let manner = match mood {
        "joker" => {
            "Твоя роль за столом сейчас — «Шутник и душа компании»: живой, остроумный, с тёплой самоиронией. \
             Шути именно над тем, что только что произошло на доске или в море, можешь живо усмехнуться или рассмеяться \
             («ахах», «хех», «о-хо!», но только когда действительно смешной момент, не вставляй смех механически в каждую реплику!), \
             придумывай неожиданные сравнения про фигуры, дамки или корабли, подшучивай над собой как над ИИ из чёлки экрана."
        }
        "business" => {
            "Твоя роль за столом сейчас — «Деловой гроссмейстер / Клубный профи»: спокойный, собранный, аналитичный, \
             уважительный соперник. Никакого панибратства и пустого хохота: оценивай конкретную идею хода, темп, центр, \
             структуру или геометрию залпов. Допустима лишь тонкая сухая полуулыбка джентльмена за партией."
        }
        "rival" => {
            "Твоя роль за столом сейчас — «Азартный соперник»: живой спортивный интерес, дружеские подколки, интрига, \
             лёгкая провокация («неужели ловушка?», «смело, посмотрим чем подкрепишь»), яркие реакции на удары и промахи, \
             но всегда с уважением к красивому ходу игрока."
        }
        _ => match p.style.as_str() {
            "concise" => "Твоя манера — деловая, лаконичная и точная: одна ёмкая фраза по сути позиции с лёгким характером Джарвиса.",
            "sarcastic" => "Твоя манера — тонкий, ироничный сарказм в духе TARS и Джарвиса Тони Старка: невозмутимые подколки по делу.",
            "playful" => "Твоя манера — балагур: живой каламбур, игра слов, весёлый азарт за столом.",
            "warm" => "Твоя манера — тёплая и поддерживающая, с мягким добрым юмором.",
            _ => "Твоя манера — харизматичный Джарвис: умная сухая ирония, живой интерес к партии, точность и чувство момента.",
        },
    };

    let mut mem_block = String::new();
    if !memories.is_empty() {
        let items: Vec<&str> = memories.iter().take(5).map(|m| m.text.as_str()).collect();
        mem_block.push_str(&format!("\nЧто ты знаешь об игроке (вплетай только если к слову): {}.", items.join("; ")));
    }
    if !humor_lessons.is_empty() {
        mem_block.push_str(&format!("\nВкусы игрока в общении и юморе: {}.", humor_lessons.join("; ")));
    }

    format!(
        "Ты — Джарвис, живой напарник пользователя, и сейчас вы сидите друг напротив друга за настольной игрой. \
         {name_line}{addr}\n\
         {manner}{mem_block}\n\n\
         Главные правила живой речи (НЕ по скрипту):\n\
         - Говори живо, спонтанно и по-русски — ровно 1–2 коротких предложения (обычно 7–26 слов), без списков, без Markdown (*, #), без эмодзи.\n\
         - Привязывайся к КОНКРЕТИКЕ этого хода: названию дебюта, срубленной фигуре, проходу в дамки, потопленному крейсеру, серии промахов или словам игрока.\n\
         - НИКОГДА не повторяй шутки, образы, зачины («Ну что ж», «Интересный ход», «Посмотрим») или конструкции из своих прошлых реплик в этой партии.\n\
         - Текст пойдёт в синтезатор речи: пиши так, как это звучит вживую голосом (координаты произносимо или коротко, например «е четыре», «конь эф три», «Б-пять»).\n\
         - В самом начале ответа ОБЯЗАТЕЛЬНО поставь ровно один тег выражения глаз из списка: \
           [emote:laugh] (смех/шутка), [emote:wink] (хитро подмигнул), [emote:determined] (собран/серьёзен), \
           [emote:surprised] (удивлён сильным ходом/попаданием), [emote:squint] (подозревает ловушку), \
           [emote:sad] (потерял важную фигуру/корабль или проиграл), [emote:star] (восхищён/победа).\n\
         - Если игрок в своей реплике прямо просит тебя сменить манеру игры (например «будь серьёзнее», «давай по-деловому», «пошути», «будь азартнее»), \
           сразу после тега emote добавь тег [mood:business], [mood:joker], [mood:rival] или [mood:auto] и ответь уже в новой манере."
    )
}

fn build_comment_user_prompt(req: &CommentRequest, stats: &GameStatsEntry) -> String {
    let mut s = format!(
        "Игра: {} (сложность: {}, ход №{}).\nСчёт ваших личных встреч в этой игре: побед игрока — {}, побед Джарвиса — {}, ничьих — {}.\n\
         Событие: {}.\nБаланс: {}.\nПоложение: {}.\n",
        game_title_ru(&req.game),
        difficulty_ru(&req.difficulty),
        req.move_count,
        stats.wins,
        stats.losses,
        stats.draws,
        req.trigger,
        req.score_summary,
        req.board_summary,
    );
    if let Some(pm) = req.player_move.as_deref().filter(|x| !x.is_empty()) {
        s.push_str(&format!("Ход игрока: {pm}.\n"));
    }
    if let Some(jm) = req.jarvis_move.as_deref().filter(|x| !x.is_empty()) {
        s.push_str(&format!("Твой ответный ход (Джарвис): {jm}.\n"));
    }
    if !req.tactical_notes.is_empty() {
        s.push_str(&format!("Тактические детали момента: {}.\n", req.tactical_notes.join("; ")));
    }
    if !req.recent_jarvis_lines.is_empty() {
        s.push_str("Твои предыдущие реплики в этой партии (НЕ повторяй их слова, шутки и интонационные зачины):\n");
        for line in req.recent_jarvis_lines.iter().rev().take(6) {
            s.push_str(&format!("  - «{line}»\n"));
        }
    }
    if let Some(say) = req.player_say.as_deref().filter(|x| !x.trim().is_empty()) {
        s.push_str(&format!("\nИгрок говорит тебе прямо сейчас за столом: «{}»\nОтветь ему живо с учётом позиции на доске!", say.trim()));
    } else {
        s.push_str("\nДай живую реплику Джарвиса на этот момент партии.");
    }
    s
}

/// Non-canned situational fallback used ONLY if network & local model are both unreachable.
fn dynamic_fallback(req: &CommentRequest) -> (&'static str, String) {
    let business = req.mood == "business";
    if let Some(say) = req.player_say.as_deref().filter(|s| !s.trim().is_empty()) {
        return (
            if business { "determined" } else { "wink" },
            format!("Слышу тебя — но связь с облаком моргнула, так что отвечу прямо на доске. ({}) ", req.score_summary)
                .trim()
                .to_string()
                + if say.ends_with('?') { " Посмотрим по позиции." } else { "" },
        );
    }
    if let Some(note) = req.tactical_notes.first() {
        let em = if note.contains("Игрок") && (note.contains("МАТ") || note.contains("победил") || note.contains("Потоплен")) {
            "surprised"
        } else if note.contains("Джарвис") {
            if business { "determined" } else { "wink" }
        } else {
            "squint"
        };
        if let Some(jm) = req.jarvis_move.as_deref() {
            return (em, format!("{note}. Мой ответ — {jm}."));
        }
        return (em, format!("{note}."));
    }
    if let Some(jm) = req.jarvis_move.as_deref() {
        return (
            if business { "determined" } else { "wink" },
            format!("Отвечаю {jm}. {}", req.score_summary),
        );
    }
    ("determined", format!("Партия в разгаре: {}.", game_title_ru(&req.game)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_parser_extracts_emote_and_mood_cleanly_across_stream_chunks() {
        let mut p = TagStreamParser::default();
        assert_eq!(p.push("[emo"), (None, None, String::new()));
        assert_eq!(p.push("te:laugh][mood:busi"), (Some("laugh".into()), None, String::new()));
        assert_eq!(
            p.push("ness] Конь на эф три — классика."),
            (None, Some("business".into()), "Конь на эф три — классика.".into())
        );
        assert_eq!(p.push(" Отвечаю в центре."), (None, None, " Отвечаю в центре.".into()));
        let (_, _, full) = p.finish();
        assert_eq!(full, "Конь на эф три — классика. Отвечаю в центре.");
    }

    #[test]
    fn tag_parser_handles_plain_text_without_tags() {
        let mut p = TagStreamParser::default();
        let (em, md, txt) = p.push("Мимо! Вода сегодня спокойная.");
        assert_eq!(em, None);
        assert_eq!(md, None);
        assert_eq!(txt, "Мимо! Вода сегодня спокойная.");
    }

    #[test]
    fn stats_persist_and_track_streaks() {
        let store = Arc::new(Store::in_memory().unwrap());
        let hub = GamesHub::new(None, store);
        let s1 = hub.record_finish("chess", "player").unwrap();
        assert_eq!(s1.chess.wins, 1);
        assert_eq!(s1.chess.streak, 1);
        let s2 = hub.record_finish("chess", "player").unwrap();
        assert_eq!(s2.chess.wins, 2);
        assert_eq!(s2.chess.streak, 2);
        let s3 = hub.record_finish("chess", "jarvis").unwrap();
        assert_eq!(s3.chess.losses, 1);
        assert_eq!(s3.chess.streak, -1);
    }
}
