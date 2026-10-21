//! Autonomous agent runtime.
//!
//! One turn = Observe (context block) → Reason (model) → Act (tools, with
//! permission checks) → Verify (tools return read-back state; the model sees
//! ok/summary/data) → Recover (failures go back to the model, bounded by
//! `max_steps`) → Report (final text). Simple requests whose tool calls are
//! all fast-path and succeed end without a second model round-trip.

pub mod events;
mod prompt;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::ai::types::{ChatMessage, Part, Role, StreamDelta, ToolCall};
use crate::ai::{self, OpenAiCompatible};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::settings::Settings;
use crate::tools::{Registry, Risk, ToolCtx, ToolOutput};
use events::{AgentEvent, EVT_AGENT};

/// Messages of history sent to the model (older ones stay in SQLite).
const HISTORY_WINDOW: usize = 48;
/// Tool results larger than this are cut before going back to the model.
const MAX_TOOL_RESULT: usize = 14_000;
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub turn_id: String,
    pub conversation_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub configured: bool,
    pub provider: String,
    pub model: String,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub role: &'static str,
    pub text: String,
    pub tool_title: Option<String>,
    pub tool_ok: Option<bool>,
    pub created_at: String,
}

struct ActiveTurn {
    turn_id: String,
    cancel: CancellationToken,
}

struct Inner {
    conversation_id: String,
    history: Vec<ChatMessage>,
    loaded: bool,
    active: Option<ActiveTurn>,
    confirms: HashMap<String, oneshot::Sender<bool>>,
    /// Moderate-risk tools the user already approved in this conversation.
    approved_tools: HashSet<String>,
}

pub struct Agent {
    app: AppHandle,
    registry: Arc<Registry>,
    inner: Mutex<Inner>,
}

struct Turn {
    id: String,
    cancel: CancellationToken,
    attachments: Arc<Vec<String>>,
    settings: Settings,
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];

fn ext_of(p: &str) -> String {
    Path::new(p).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}… [обрезано]")
}

impl Agent {
    pub fn new(app: AppHandle, registry: Arc<Registry>) -> Arc<Self> {
        let store = &app.state::<AppState>().store;
        let conversation_id = store.latest_conversation().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Arc::new(Agent {
            app,
            registry,
            inner: Mutex::new(Inner {
                conversation_id,
                history: Vec::new(),
                loaded: false,
                active: None,
                confirms: HashMap::new(),
                approved_tools: HashSet::new(),
            }),
        })
    }

    fn emit(&self, ev: AgentEvent) {
        let _ = self.app.emit(EVT_AGENT, ev);
    }

    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    pub fn status(&self) -> AgentStatus {
        let s = self.state().store.settings();
        let preset = crate::store::settings::preset(&s.provider.preset);
        match ai::client_from_settings(&s) {
            Ok(c) => AgentStatus { configured: true, provider: preset.label.into(), model: c.model, problem: None },
            Err(e) => AgentStatus {
                configured: false,
                provider: preset.label.into(),
                model: s.provider.model,
                problem: Some(e.to_string()),
            },
        }
    }

    fn ensure_loaded(&self) {
        let (needs, conv) = {
            let i = self.inner.lock();
            (!i.loaded, i.conversation_id.clone())
        };
        if !needs {
            return;
        }
        let msgs = self.state().store.messages(&conv).unwrap_or_default();
        let mut i = self.inner.lock();
        if !i.loaded {
            i.history = msgs.into_iter().map(|m| m.message).collect();
            i.loaded = true;
        }
    }

    pub fn history(&self) -> Vec<HistoryItem> {
        let conv = self.inner.lock().conversation_id.clone();
        let stored = self.state().store.messages(&conv).unwrap_or_default();
        stored
            .into_iter()
            .filter_map(|m| {
                let text = m.message.joined_text();
                match m.message.role {
                    Role::User if !text.starts_with("[tool-image]") => {
                        Some(HistoryItem { role: "user", text, tool_title: None, tool_ok: None, created_at: m.created_at })
                    }
                    Role::Assistant if !text.trim().is_empty() => {
                        Some(HistoryItem { role: "assistant", text, tool_title: None, tool_ok: None, created_at: m.created_at })
                    }
                    Role::Tool => {
                        let summary = serde_json::from_str::<Value>(&text)
                            .ok()
                            .and_then(|v| v.get("summary").or_else(|| v.get("error")).and_then(Value::as_str).map(str::to_string))
                            .unwrap_or_default();
                        Some(HistoryItem { role: "tool", text: summary, tool_title: m.tool_title, tool_ok: m.tool_ok, created_at: m.created_at })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    pub fn new_conversation(&self) -> AppResult<()> {
        let mut i = self.inner.lock();
        if i.active.is_some() {
            return Err(AppError::invalid("Дождитесь окончания текущей задачи"));
        }
        i.conversation_id = uuid::Uuid::new_v4().to_string();
        i.history.clear();
        i.loaded = true;
        i.approved_tools.clear();
        Ok(())
    }

    pub fn cancel(&self) {
        let mut i = self.inner.lock();
        if let Some(t) = &i.active {
            tracing::info!(turn = %t.turn_id, "cancel requested");
            t.cancel.cancel();
        }
        for (_, tx) in i.confirms.drain() {
            let _ = tx.send(false);
        }
    }

    pub fn confirm(&self, call_id: &str, approved: bool) -> AppResult<()> {
        let tx = self.inner.lock().confirms.remove(call_id);
        match tx {
            Some(tx) => {
                let _ = tx.send(approved);
                Ok(())
            }
            None => Err(AppError::not_found("Запрос подтверждения уже неактуален")),
        }
    }

    pub fn send(self: &Arc<Self>, text: String, attachments: Vec<String>) -> AppResult<SendResult> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(AppError::invalid("Пустое сообщение"));
        }
        let settings = self.state().store.settings();
        let client = ai::client_from_settings(&settings)?;
        self.ensure_loaded();

        let turn = {
            let mut i = self.inner.lock();
            if i.active.is_some() {
                return Err(AppError::invalid("Джарвис уже выполняет задачу — дождитесь или остановите её"));
            }
            let t = Turn {
                id: uuid::Uuid::new_v4().to_string(),
                cancel: CancellationToken::new(),
                attachments: Arc::new(attachments),
                settings,
            };
            i.active = Some(ActiveTurn { turn_id: t.id.clone(), cancel: t.cancel.clone() });
            t
        };
        let conversation_id = self.inner.lock().conversation_id.clone();
        let result = SendResult { turn_id: turn.id.clone(), conversation_id: conversation_id.clone() };

        let agent = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            agent.run_turn(turn, conversation_id, text, client).await;
        });
        Ok(result)
    }

    fn user_message(&self, text: &str, attachments: &[String]) -> ChatMessage {
        let mut body = text.to_string();
        if !attachments.is_empty() {
            body.push_str("\n\n[Прикреплённые файлы — можно читать без подтверждения: ");
            body.push_str(&attachments.join("; "));
            body.push(']');
        }
        let mut parts = vec![Part::Text { text: body }];
        for a in attachments {
            if IMAGE_EXTS.contains(&ext_of(a).as_str()) {
                if let Ok(bytes) = std::fs::read(a) {
                    if let Ok(url) = crate::tools::image_data_url(&bytes) {
                        parts.push(Part::Image { data_url: url });
                    }
                }
            }
        }
        ChatMessage { role: Role::User, parts, tool_calls: vec![], tool_call_id: None }
    }

    /// History window for the model: starts at a user message (never at an
    /// orphaned tool result) and keeps images only in the newest message.
    fn context(&self, system: String) -> Vec<ChatMessage> {
        let i = self.inner.lock();
        let h = &i.history;
        let mut start = h.len().saturating_sub(HISTORY_WINDOW);
        while start < h.len() && h[start].role != Role::User {
            start += 1;
        }
        let last_image = h.iter().rposition(ChatMessage::has_images);
        let mut out = vec![ChatMessage::text(Role::System, system)];
        for (idx, m) in h.iter().enumerate().skip(start) {
            if m.has_images() && Some(idx) != last_image {
                let mut m = m.clone();
                m.parts.retain(|p| matches!(p, Part::Text { .. }));
                m.parts.push(Part::Text { text: " [изображение опущено]".into() });
                out.push(m);
            } else {
                out.push(m.clone());
            }
        }
        out
    }

    fn push_history(&self, conversation_id: &str, msg: ChatMessage, tool_title: Option<&str>, tool_ok: Option<bool>, persist: bool) {
        if persist {
            let store = &self.state().store;
            // Screenshots and other images are not written to disk.
            let mut stored = msg.clone();
            if stored.has_images() {
                stored.parts.retain(|p| matches!(p, Part::Text { .. }));
            }
            if let Err(e) = store.append_message(conversation_id, &stored, tool_title, tool_ok) {
                tracing::warn!("history write failed: {e}");
            }
        }
        self.inner.lock().history.push(msg);
    }

    async fn run_turn(self: Arc<Self>, turn: Turn, conversation_id: String, text: String, client: OpenAiCompatible) {
        let started = Instant::now();
        self.emit(AgentEvent::TurnStarted { turn_id: turn.id.clone(), conversation_id: conversation_id.clone() });
        let persist = turn.settings.privacy.store_history;
        let store = self.state().store.clone();
        if persist {
            let _ = store.create_conversation(&conversation_id);
            let _ = store.task_started(&turn.id, &conversation_id, &text);
        }
        let user = self.user_message(&text, &turn.attachments);
        self.push_history(&conversation_id, user, None, None, persist);

        let mut steps = 0u32;
        let outcome = self.turn_loop(&turn, &conversation_id, &client, &mut steps).await;

        let (status, error) = match &outcome {
            Ok(()) => ("completed", None),
            Err(AppError::Cancelled) => ("cancelled", None),
            Err(e) => ("failed", Some(e.to_string())),
        };
        if persist {
            let _ = store.task_finished(&turn.id, status, steps, error.as_deref());
        }
        tracing::info!(turn = %turn.id, status, steps, ms = started.elapsed().as_millis() as u64, "turn finished");
        {
            let mut i = self.inner.lock();
            i.active = None;
            i.confirms.clear();
        }
        self.emit(AgentEvent::TurnFinished { turn_id: turn.id.clone(), status, error });
    }

    async fn turn_loop(&self, turn: &Turn, conversation_id: &str, client: &OpenAiCompatible, steps: &mut u32) -> AppResult<()> {
        let specs = self.registry.specs();
        let names = self.registry.names();
        let vision_model = turn.settings.provider.vision_model.trim().to_string();
        let persist = turn.settings.privacy.store_history;
        loop {
            if *steps >= turn.settings.agent.max_steps.max(1) {
                return Err(AppError::internal(format!(
                    "Остановился после {} шагов, чтобы не зациклиться. Уточните задачу или разбейте её на части.",
                    steps
                )));
            }
            *steps += 1;
            self.emit(AgentEvent::Phase { turn_id: turn.id.clone(), phase: "thinking" });

            let system = {
                let st = self.state();
                let memories = st.store.memories().unwrap_or_default();
                prompt::system_prompt(&turn.settings, &st.history, &memories, &names)
            };
            let messages = self.context(system);
            let use_vision = !vision_model.is_empty() && messages.last().is_some_and(ChatMessage::has_images);
            let model = if use_vision { client.with_model(&vision_model) } else { client.clone() };

            let turn_id = turn.id.clone();
            let app = self.app.clone();
            let completion = model
                .complete(&messages, &specs, &turn.cancel, None, move |d| {
                    if let StreamDelta::Text(t) = d {
                        let _ = app.emit(EVT_AGENT, AgentEvent::TextDelta { turn_id: turn_id.clone(), text: t });
                    }
                })
                .await?;
            let msg = completion.message;
            let calls = msg.tool_calls.clone();
            self.push_history(conversation_id, msg, None, None, persist);

            if calls.is_empty() {
                return Ok(());
            }
            self.emit(AgentEvent::Phase { turn_id: turn.id.clone(), phase: "acting" });

            let results = self.execute_calls(turn, &calls).await?;
            let mut images = Vec::new();
            let mut all_fast_ok = true;
            let mut summaries = Vec::new();
            for (call, fast, res) in &results {
                let (content, title, ok) = match res {
                    Ok(out) => {
                        images.extend(out.images.iter().cloned().map(|u| (call.name.clone(), u)));
                        summaries.push(out.summary.clone());
                        all_fast_ok &= *fast && out.ok;
                        let body = json!({ "ok": out.ok, "summary": out.summary, "data": out.data });
                        (truncate_chars(&body.to_string(), MAX_TOOL_RESULT), self.title_for(call), out.ok)
                    }
                    Err(e) => {
                        all_fast_ok = false;
                        let body = json!({ "ok": false, "error": e.to_string(), "code": e.code() });
                        (body.to_string(), self.title_for(call), false)
                    }
                };
                self.push_history(conversation_id, ChatMessage::tool_result(&call.id, content), Some(&title), Some(ok), persist);
            }

            if !images.is_empty() {
                let mut parts = vec![Part::Text { text: "[tool-image] Изображения, полученные инструментами на этом шаге:".into() }];
                for (tool, url) in images {
                    parts.push(Part::Text { text: format!("\n{tool}:") });
                    parts.push(Part::Image { data_url: url });
                }
                self.push_history(
                    conversation_id,
                    ChatMessage { role: Role::User, parts, tool_calls: vec![], tool_call_id: None },
                    None,
                    None,
                    persist,
                );
                all_fast_ok = false;
            }

            // Fast path: a simple request whose actions all succeeded and
            // whose verified summaries already answer it.
            if *steps == 1 && all_fast_ok {
                let text = format!("Готово: {}.", summaries.join("; ").trim_end_matches('.'));
                self.emit(AgentEvent::TextDelta { turn_id: turn.id.clone(), text: text.clone() });
                self.push_history(conversation_id, ChatMessage::text(Role::Assistant, text), None, None, persist);
                return Ok(());
            }
        }
    }

    fn title_for(&self, call: &ToolCall) -> String {
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
        self.registry.get(&call.name).map(|t| (t.title)(&args)).unwrap_or_else(|| call.name.clone())
    }

    /// Policy adjustments on top of a tool's own risk assessment.
    fn effective_risk(&self, turn: &Turn, name: &str, base: Risk, args: &Value) -> Risk {
        match name {
            "file_read" => {
                let p = args.get("path").and_then(Value::as_str).unwrap_or("").to_lowercase();
                let attached = turn.attachments.iter().any(|a| {
                    let a = a.to_lowercase();
                    p == a || p.starts_with(&format!("{}\\", a.trim_end_matches('\\')))
                });
                if attached { Risk::Low } else { base }
            }
            "clipboard_read" if turn.settings.privacy.allow_clipboard_read => Risk::Low,
            _ => base,
        }
    }

    async fn execute_calls(&self, turn: &Turn, calls: &[ToolCall]) -> AppResult<Vec<(ToolCall, bool, AppResult<ToolOutput>)>> {
        // Parse and assess everything first.
        let mut prepared = Vec::new();
        for call in calls {
            let args: Result<Value, _> = serde_json::from_str(&call.arguments);
            let tool = self.registry.get(&call.name);
            let (risk, parallel, fast) = match (&tool, &args) {
                (Some(t), Ok(a)) => (self.effective_risk(turn, t.name, (t.risk)(a), a), t.parallel_safe, t.fast_path),
                _ => (Risk::Low, false, false),
            };
            prepared.push((call.clone(), args, risk, parallel, fast));
        }

        let all_parallel = prepared.len() > 1 && prepared.iter().all(|(_, a, r, p, _)| a.is_ok() && *p && *r == Risk::Low);
        if all_parallel {
            let futs = prepared.into_iter().map(|(call, args, risk, _, fast)| async move {
                let res = self.execute_one(turn, &call, args.unwrap(), risk).await;
                (call, fast, res)
            });
            let out = futures_util::future::join_all(futs).await;
            if turn.cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            return Ok(out);
        }

        let mut out = Vec::new();
        for (call, args, risk, _, fast) in prepared {
            let res = match args {
                Ok(a) => self.execute_one(turn, &call, a, risk).await,
                Err(e) => Err(AppError::invalid(format!("Аргументы не являются корректным JSON: {e}"))),
            };
            if matches!(res, Err(AppError::Cancelled)) || turn.cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            out.push((call, fast, res));
        }
        Ok(out)
    }

    async fn ask_confirmation(&self, turn: &Turn, call: &ToolCall, title: &str, risk: Risk, args: &Value) -> AppResult<bool> {
        let details = match args.get("command").and_then(Value::as_str) {
            Some(cmd) => cmd.to_string(),
            None => serde_json::to_string_pretty(args).unwrap_or_default(),
        };
        let (tx, rx) = oneshot::channel();
        self.inner.lock().confirms.insert(call.id.clone(), tx);
        self.emit(AgentEvent::ConfirmationRequired {
            turn_id: turn.id.clone(),
            call_id: call.id.clone(),
            tool: call.name.clone(),
            title: title.to_string(),
            risk: risk.as_str(),
            details: truncate_chars(&details, 2000),
        });
        let approved = tokio::select! {
            _ = turn.cancel.cancelled() => return Err(AppError::Cancelled),
            r = tokio::time::timeout(CONFIRM_TIMEOUT, rx) => matches!(r, Ok(Ok(true))),
        };
        self.inner.lock().confirms.remove(&call.id);
        Ok(approved)
    }

    async fn execute_one(&self, turn: &Turn, call: &ToolCall, args: Value, risk: Risk) -> AppResult<ToolOutput> {
        let Some(tool) = self.registry.get(&call.name) else {
            return Err(AppError::not_found(format!("Инструмента «{}» нет. Доступные: {}", call.name, self.registry.names().join(", "))));
        };
        let title = (tool.title)(&args);
        let store = self.state().store.clone();
        let args_text = truncate_chars(&args.to_string(), 4000);

        let needs_confirm = match risk {
            Risk::High => true,
            Risk::Moderate => turn.settings.agent.confirm_moderate && !self.inner.lock().approved_tools.contains(tool.name),
            Risk::Low => false,
        };
        if needs_confirm {
            let approved = self.ask_confirmation(turn, call, &title, risk, &args).await?;
            store.audit(Some(&turn.id), tool.name, risk.as_str(), &args_text, if approved { "approved" } else { "declined" }, None);
            if !approved {
                self.emit(AgentEvent::ToolFinished {
                    turn_id: turn.id.clone(),
                    call_id: call.id.clone(),
                    ok: false,
                    summary: "Отклонено пользователем".into(),
                    error: None,
                });
                return Ok(ToolOutput::fail("Пользователь отклонил это действие — не повторяй его без новой просьбы", json!({ "declined": true })));
            }
            if risk == Risk::Moderate {
                self.inner.lock().approved_tools.insert(tool.name.to_string());
            }
        }

        self.emit(AgentEvent::ToolStarted {
            turn_id: turn.id.clone(),
            call_id: call.id.clone(),
            tool: call.name.clone(),
            title: title.clone(),
            args: args.clone(),
        });

        let app = self.app.clone();
        let (turn_id, call_id) = (turn.id.clone(), call.id.clone());
        let ctx = ToolCtx {
            app: self.app.clone(),
            cancel: turn.cancel.clone(),
            task_id: turn.id.clone(),
            attachments: turn.attachments.clone(),
            progress: Arc::new(move |line: &str| {
                let _ = app.emit(
                    EVT_AGENT,
                    AgentEvent::ToolProgress { turn_id: turn_id.clone(), call_id: call_id.clone(), line: truncate_chars(line, 400) },
                );
            }),
        };
        let started = Instant::now();
        let res = self.registry.run(&call.name, ctx, args).await;
        tracing::info!(tool = %call.name, ms = started.elapsed().as_millis() as u64, ok = res.as_ref().map(|o| o.ok).unwrap_or(false), "tool finished");

        let (ok, summary, error) = match &res {
            Ok(o) => (o.ok, o.summary.clone(), None),
            Err(AppError::Cancelled) => (false, "Отменено".to_string(), None),
            Err(e) => (false, String::new(), Some(e.to_string())),
        };
        if risk >= Risk::Moderate {
            store.audit(Some(&turn.id), tool.name, risk.as_str(), &args_text, "executed", Some(if ok { &summary } else { error.as_deref().unwrap_or(&summary) }));
        }
        self.emit(AgentEvent::ToolFinished { turn_id: turn.id.clone(), call_id: call.id.clone(), ok, summary, error });
        res
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncation_respects_char_boundaries() {
        let s = "привет".repeat(10);
        let t = truncate_chars(&s, 7);
        assert!(t.starts_with("привет"));
        assert!(t.ends_with("[обрезано]"));
    }
}
