//! OpenAI-compatible Chat Completions client with streaming tool calls.
//! Used for Qwen (Model Studio compatible mode), OpenAI, OpenRouter and
//! local servers (Ollama). Provider quirks handled here:
//!  * tool-call fragments are merged by `index`; empty `id`/`name`
//!    fragments never overwrite earlier values (DashScope sends `"id":""`
//!    in late chunks);
//!  * some servers omit `index` and send a whole call per chunk;
//!  * DashScope hybrid models think by default — `enable_thinking: false`
//!    keeps tool-calling latency low;
//!  * usage arrives in a final chunk with empty `choices`.

use std::time::{Duration, Instant};

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::types::{ChatMessage, Completion, Part, Role, StreamDelta, ToolCall, ToolSpec, Usage};
use crate::error::{AppError, AppResult};

const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct OpenAiCompatible {
    client: reqwest::Client,
    pub base_url: String,
    api_key: Option<String>,
    pub model: String,
    pub dashscope: bool,
    pub temperature: f32,
}

impl std::fmt::Debug for OpenAiCompatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.debug_struct("OpenAiCompatible").field("base_url", &self.base_url).field("model", &self.model).finish()
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(90))
        .user_agent(concat!("JARVIS/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

fn wire_message(m: &ChatMessage) -> Value {
    let text = m.joined_text();
    match m.role {
        Role::System => json!({ "role": "system", "content": text }),
        Role::User => {
            if m.has_images() {
                let content: Vec<Value> = m
                    .parts
                    .iter()
                    .map(|p| match p {
                        Part::Text { text } => json!({ "type": "text", "text": text }),
                        Part::Image { data_url } => json!({ "type": "image_url", "image_url": { "url": data_url } }),
                    })
                    .collect();
                json!({ "role": "user", "content": content })
            } else {
                json!({ "role": "user", "content": text })
            }
        }
        Role::Assistant => {
            let mut v = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text) } });
            if !m.tool_calls.is_empty() {
                v["tool_calls"] = m
                    .tool_calls
                    .iter()
                    .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments } }))
                    .collect();
            }
            v
        }
        Role::Tool => json!({ "role": "tool", "tool_call_id": m.tool_call_id.clone().unwrap_or_default(), "content": text }),
    }
}

/// Removes inline `<think>…</think>` blocks (servers without a reasoning
/// parser) and the blank lines some models emit before the answer.
#[derive(Default)]
struct ThinkFilter {
    inside: bool,
    pending: String,
}

impl ThinkFilter {
    fn feed(&mut self, chunk: &str, at_start: bool) -> String {
        self.pending.push_str(chunk);
        let mut out = String::new();
        loop {
            if self.inside {
                match self.pending.find("</think>") {
                    Some(i) => {
                        self.pending.drain(..i + "</think>".len());
                        self.inside = false;
                    }
                    None => return out,
                }
            } else {
                match self.pending.find("<think>") {
                    Some(i) => {
                        out.push_str(&self.pending[..i]);
                        self.pending.drain(..i + "<think>".len());
                        self.inside = true;
                    }
                    None => {
                        // Hold back a possible partial "<think" at the end.
                        let keep = (1..7).rev().find(|n| self.pending.len() >= *n && self.pending.is_char_boundary(self.pending.len() - n) && "<think>".starts_with(&self.pending[self.pending.len() - n..])).unwrap_or(0);
                        let cut = self.pending.len() - keep;
                        out.push_str(&self.pending[..cut]);
                        self.pending.drain(..cut);
                        break;
                    }
                }
            }
        }
        if at_start {
            out.trim_start().to_string()
        } else {
            out
        }
    }

    /// Text held back at the end of the stream (never an open think block).
    fn finish(&mut self) -> String {
        if self.inside { String::new() } else { std::mem::take(&mut self.pending) }
    }
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

fn error_from_response(status: reqwest::StatusCode, body: &str, model: &str) -> AppError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let detail = parsed
        .as_ref()
        .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")))
        .and_then(Value::as_str)
        .map(|s| s.chars().take(300).collect::<String>());
    match status.as_u16() {
        401 | 403 => AppError::Provider(
            "Провайдер отклонил API-ключ. Проверьте ключ и регион (ключи Model Studio привязаны к региону).".into(),
        ),
        404 => AppError::Provider(format!(
            "Модель «{model}» или адрес API не найдены{}",
            detail.map(|d| format!(": {d}")).unwrap_or_default()
        )),
        s if s >= 500 => AppError::Provider(format!("Провайдер временно недоступен (HTTP {s})")),
        s => AppError::Provider(format!("Запрос отклонён (HTTP {s}){}", detail.map(|d| format!(": {d}")).unwrap_or_default())),
    }
}

fn retryable(e: &AppError) -> bool {
    match e {
        AppError::Network(_) | AppError::Timeout(_) => true,
        AppError::Provider(m) => m.contains("временно"),
        _ => false,
    }
}

impl OpenAiCompatible {
    pub fn new(base_url: String, api_key: Option<String>, model: String, dashscope: bool, temperature: f32) -> Self {
        OpenAiCompatible { client: http_client(), base_url, api_key, model, dashscope, temperature }
    }

    pub fn with_model(&self, model: &str) -> Self {
        let mut c = self.clone();
        c.model = model.to_string();
        c
    }

    fn body(&self, messages: &[ChatMessage], tools: &[ToolSpec], max_tokens: Option<u32>) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": messages.iter().map(wire_message).collect::<Vec<_>>(),
            "stream": true,
            "stream_options": { "include_usage": true },
            "temperature": self.temperature,
        });
        if !tools.is_empty() {
            body["tools"] = tools
                .iter()
                .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
                .collect();
            // Qwen does not support tool_choice "required"; "auto" is the default.
            body["parallel_tool_calls"] = Value::Bool(true);
        }
        if self.dashscope {
            body["enable_thinking"] = Value::Bool(false);
        }
        if let Some(n) = max_tokens {
            body["max_tokens"] = json!(n);
        }
        body
    }

    /// Stream one completion. Retries transient failures that happen before
    /// any output was produced.
    pub async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
        cancel: &CancellationToken,
        max_tokens: Option<u32>,
        mut on_delta: impl FnMut(StreamDelta) + Send,
    ) -> AppResult<Completion> {
        let body = self.body(messages, tools, max_tokens);
        let mut attempt = 0;
        let mut limited = 0;
        loop {
            // Pace requests under a learned per-minute limit instead of
            // hitting 429 again.
            let pace = rate::wait_before(&self.base_url);
            if !pace.is_zero() {
                on_delta(StreamDelta::Waiting(pace.as_secs().max(1)));
                tokio::select! {
                    _ = cancel.cancelled() => return Err(AppError::Cancelled),
                    _ = tokio::time::sleep(pace) => {}
                }
            }
            rate::record(&self.base_url);
            let mut produced = false;
            let res = self.stream_once(&body, cancel, &mut |d| {
                produced = true;
                on_delta(d)
            })
            .await;
            match res {
                Err(AppError::RateLimited(secs)) if !produced && limited < 3 && !cancel.is_cancelled() => {
                    limited += 1;
                    rate::learn(&self.base_url);
                    tracing::warn!(secs, limited, "provider rate limit, waiting");
                    on_delta(StreamDelta::Waiting(secs));
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(AppError::Cancelled),
                        _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                    }
                }
                Err(e) if !produced && attempt < 2 && retryable(&e) && !cancel.is_cancelled() => {
                    attempt += 1;
                    let wait = Duration::from_millis(if attempt == 1 { 900 } else { 2800 });
                    tracing::warn!(attempt, "LLM request failed, retrying: {e}");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(AppError::Cancelled),
                        _ = tokio::time::sleep(wait) => {}
                    }
                }
                other => return other,
            }
        }
    }

    async fn stream_once(
        &self,
        body: &Value,
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    ) -> AppResult<Completion> {
        let started = Instant::now();
        let mut req = self.client.post(format!("{}/chat/completions", self.base_url)).json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            r = tokio::time::timeout(FIRST_BYTE_TIMEOUT, req.send()) => r
                .map_err(|_| AppError::Timeout("Провайдер не ответил за 60 секунд".into()))??,
        };
        let status = resp.status();
        if status.as_u16() == 429 {
            let secs = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(20);
            return Err(AppError::RateLimited(secs.clamp(1, 120)));
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(error_from_response(status, &text, &self.model));
        }

        let mut stream = resp.bytes_stream().eventsource();
        let mut text = String::new();
        let mut calls: Vec<PartialCall> = Vec::new();
        let mut usage = None;
        let mut finish_reason = None;
        let mut first_token_ms = None;
        let mut think = ThinkFilter::default();

        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
                n = tokio::time::timeout(IDLE_TIMEOUT, stream.next()) => n,
            };
            let ev = match next {
                Err(_) => return Err(AppError::Timeout("Поток ответа модели прервался".into())),
                Ok(None) => break,
                Ok(Some(Err(e))) => return Err(AppError::Network(format!("Ошибка потока ответа: {e}"))),
                Ok(Some(Ok(ev))) => ev,
            };
            if ev.data.trim() == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<Value>(&ev.data) else { continue };
            if let Some(err) = chunk.get("error") {
                let msg = err.get("message").and_then(Value::as_str).unwrap_or("неизвестная ошибка");
                return Err(AppError::Provider(format!("Ошибка модели: {}", msg.chars().take(300).collect::<String>())));
            }
            if let Some(u) = chunk.get("usage").filter(|u| !u.is_null()) {
                usage = Some(Usage {
                    prompt_tokens: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
                    completion_tokens: u.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
                });
            }
            let Some(choice) = chunk.pointer("/choices/0") else { continue };
            if let Some(fr) = choice.get("finish_reason").and_then(Value::as_str) {
                finish_reason = Some(fr.to_string());
            }
            let Some(delta) = choice.get("delta") else { continue };

            if let Some(t) = delta.get("content").and_then(Value::as_str) {
                let visible = think.feed(t, text.is_empty());
                if !visible.is_empty() {
                    first_token_ms.get_or_insert(started.elapsed().as_millis() as u64);
                    text.push_str(&visible);
                    on_delta(StreamDelta::Text(visible));
                }
            }
            if delta.get("reasoning_content").and_then(Value::as_str).is_some_and(|s| !s.is_empty()) {
                first_token_ms.get_or_insert(started.elapsed().as_millis() as u64);
            }
            if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
                first_token_ms.get_or_insert(started.elapsed().as_millis() as u64);
                for tc in tcs {
                    let index = tc.get("index").and_then(Value::as_u64).map(|i| i as usize).unwrap_or(calls.len());
                    if calls.len() <= index {
                        calls.resize_with(index + 1, PartialCall::default);
                    }
                    let slot = &mut calls[index];
                    if let Some(id) = tc.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                        if slot.id.is_empty() {
                            slot.id = id.to_string();
                        }
                    }
                    if let Some(f) = tc.get("function") {
                        if let Some(name) = f.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                            if slot.name.is_empty() {
                                slot.name = name.to_string();
                                on_delta(StreamDelta::ToolCallStarted { index, name: slot.name.clone() });
                            }
                        }
                        match f.get("arguments") {
                            Some(Value::String(a)) => slot.args.push_str(a),
                            // Some servers send already-parsed arguments.
                            Some(v @ Value::Object(_)) => slot.args = v.to_string(),
                            _ => {}
                        }
                    }
                }
            }
        }

        let tail = think.finish();
        if !tail.is_empty() {
            text.push_str(&tail);
            on_delta(StreamDelta::Text(tail));
        }

        let tool_calls: Vec<ToolCall> = calls
            .into_iter()
            .enumerate()
            .filter(|(_, c)| !c.name.is_empty())
            .map(|(i, c)| ToolCall {
                id: if c.id.is_empty() { format!("call_{i}_{}", uuid::Uuid::new_v4().simple()) } else { c.id },
                name: c.name,
                arguments: if c.args.trim().is_empty() { "{}".into() } else { c.args },
            })
            .collect();

        tracing::debug!(
            model = %self.model,
            ms = started.elapsed().as_millis() as u64,
            first_token_ms,
            tools = tool_calls.len(),
            ?usage,
            "completion finished"
        );

        Ok(Completion {
            message: ChatMessage {
                role: Role::Assistant,
                parts: if text.is_empty() { Vec::new() } else { vec![Part::Text { text }] },
                tool_calls,
                tool_call_id: None,
            },
            usage,
            finish_reason,
            first_token_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_with_tool_calls_wire_format() {
        let m = ChatMessage {
            role: Role::Assistant,
            parts: vec![],
            tool_calls: vec![ToolCall { id: "c1".into(), name: "system_volume".into(), arguments: "{\"percent\":30}".into() }],
            tool_call_id: None,
        };
        let w = wire_message(&m);
        assert!(w["content"].is_null());
        assert_eq!(w["tool_calls"][0]["function"]["name"], "system_volume");
        assert_eq!(w["tool_calls"][0]["type"], "function");
    }

    #[test]
    fn user_with_image_uses_content_array() {
        let m = ChatMessage {
            role: Role::User,
            parts: vec![Part::Text { text: "что тут?".into() }, Part::Image { data_url: "data:image/jpeg;base64,AA".into() }],
            tool_calls: vec![],
            tool_call_id: None,
        };
        let w = wire_message(&m);
        assert_eq!(w["content"][1]["type"], "image_url");
    }

    /// Serve one scripted SSE response on a local port and return the base URL
    /// plus a handle yielding the raw request the client sent.
    async fn mock_sse(chunks: Vec<String>) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            // Read headers + body (Content-Length).
            loop {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(pos) = text.find("\r\n\r\n") {
                    let len = text[..pos]
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if buf.len() >= pos + 4 + len {
                        break;
                    }
                }
            }
            let mut resp = String::from("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n");
            for c in chunks {
                resp.push_str(&format!("data: {c}\n\n"));
            }
            resp.push_str("data: [DONE]\n\n");
            sock.write_all(resp.as_bytes()).await.unwrap();
            sock.shutdown().await.unwrap();
            String::from_utf8_lossy(&buf).to_string()
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn streams_fragmented_parallel_tool_calls() {
        let chunks = vec![
            r#"{"choices":[{"delta":{"content":"Сейчас "},"index":0}]}"#.to_string(),
            r#"{"choices":[{"delta":{"content":"сделаю."},"index":0}]}"#.to_string(),
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"system_volume","arguments":""}}]},"index":0}]}"#.to_string(),
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"","function":{"arguments":"{\"action\":\"set\","}}]},"index":0}]}"#.to_string(),
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"app_launch","arguments":"{\"query\":"}}]},"index":0}]}"#.to_string(),
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"","function":{"arguments":"\"percent\":30}"}}]},"index":0}]}"#.to_string(),
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"","function":{"arguments":"\"VS Code\"}"}}]},"finish_reason":"tool_calls","index":0}]}"#.to_string(),
            r#"{"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":40}}"#.to_string(),
        ];
        let (base, server) = mock_sse(chunks).await;
        let client = OpenAiCompatible::new(base, Some("test-key".into()), "qwen3.8-flash".into(), true, 0.3);
        let cancel = CancellationToken::new();
        let mut deltas = String::new();
        let tools = vec![ToolSpec { name: "system_volume".into(), description: "d".into(), parameters: json!({"type":"object"}) }];
        let c = client
            .complete(&[ChatMessage::text(Role::User, "громкость 30 и открой vs code")], &tools, &cancel, None, |d| {
                if let StreamDelta::Text(t) = d {
                    deltas.push_str(&t)
                }
            })
            .await
            .unwrap();
        assert_eq!(deltas, "Сейчас сделаю.");
        assert_eq!(c.message.tool_calls.len(), 2);
        assert_eq!(c.message.tool_calls[0].id, "call_a");
        assert_eq!(c.message.tool_calls[0].name, "system_volume");
        let args: Value = serde_json::from_str(&c.message.tool_calls[0].arguments).unwrap();
        assert_eq!(args["percent"], 30);
        assert_eq!(c.message.tool_calls[1].name, "app_launch");
        assert_eq!(c.usage.unwrap().completion_tokens, 40);
        assert_eq!(c.finish_reason.as_deref(), Some("tool_calls"));

        let request = server.await.unwrap();
        assert!(request.contains("authorization: Bearer test-key") || request.contains("Authorization: Bearer test-key"));
        assert!(request.contains("\"enable_thinking\":false"));
        assert!(request.contains("\"parallel_tool_calls\":true"));
    }

    #[tokio::test]
    async fn cancellation_stops_waiting() {
        // A server that accepts but never answers.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _keep = tokio::spawn(async move {
            let (_s, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let client = OpenAiCompatible::new(format!("http://{addr}"), None, "m".into(), false, 0.3);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            c2.cancel();
        });
        let started = Instant::now();
        let r = client.complete(&[ChatMessage::text(Role::User, "x")], &[], &cancel, None, |_| {}).await;
        assert!(matches!(r, Err(AppError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn think_filter_strips_blocks_and_leading_blank_lines() {
        let mut f = ThinkFilter::default();
        let mut out = String::new();
        for (i, c) in ["

", "<thi", "nk>plan the", " call</thi", "nk>
Готов", "о"].iter().enumerate() {
            out += &f.feed(c, out.is_empty() && i < 10);
        }
        out += &f.finish();
        assert_eq!(out, "Готово");
        let mut f = ThinkFilter::default();
        assert_eq!(f.feed("a < b", false), "a < b");
    }

    #[test]
    fn learns_rate_limit_and_paces() {
        let key = "test://rate";
        rate::reset(key);
        for _ in 0..5 {
            assert!(rate::wait_before(key).is_zero());
            rate::record(key);
        }
        // The 5th request was rejected with 429: the limit becomes 4/min.
        rate::learn(key);
        let wait = rate::wait_before(key);
        assert!(wait > Duration::from_secs(55) && wait <= Duration::from_secs(60), "{wait:?}");
        rate::reset(key);
    }

    #[test]
    fn errors_are_user_readable_and_keyless() {
        let e = error_from_response(reqwest::StatusCode::UNAUTHORIZED, "{\"error\":{\"message\":\"Incorrect API key sk-123\"}}", "m");
        assert!(!e.to_string().contains("sk-123"));
        let e = error_from_response(reqwest::StatusCode::NOT_FOUND, "{\"error\":{\"message\":\"model not found\"}}", "qwen-x");
        assert!(e.to_string().contains("qwen-x"));
    }
}

/// Client-side pacing per endpoint. After a 429, the number of requests sent
/// in the last minute becomes the learned limit, and later requests wait for
/// a free slot instead of failing.
mod rate {
    use std::collections::{HashMap, VecDeque};
    use std::sync::LazyLock;
    use std::time::{Duration, Instant};

    use parking_lot::Mutex;

    const WINDOW: Duration = Duration::from_secs(60);

    #[derive(Default)]
    struct Gate {
        recent: VecDeque<Instant>,
        limit: Option<usize>,
    }

    static GATES: LazyLock<Mutex<HashMap<String, Gate>>> = LazyLock::new(Default::default);

    fn purge(g: &mut Gate, now: Instant) {
        while g.recent.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
            g.recent.pop_front();
        }
    }

    pub fn wait_before(key: &str) -> Duration {
        let now = Instant::now();
        let mut gates = GATES.lock();
        let g = gates.entry(key.to_string()).or_default();
        purge(g, now);
        match (g.limit, g.recent.front()) {
            (Some(limit), Some(oldest)) if g.recent.len() >= limit => (*oldest + WINDOW).saturating_duration_since(now),
            _ => Duration::ZERO,
        }
    }

    pub fn record(key: &str) {
        GATES.lock().entry(key.to_string()).or_default().recent.push_back(Instant::now());
    }

    pub fn learn(key: &str) {
        let now = Instant::now();
        let mut gates = GATES.lock();
        let g = gates.entry(key.to_string()).or_default();
        purge(g, now);
        // The rejected request is in `recent`; the server allowed one fewer.
        let allowed = g.recent.len().saturating_sub(1).max(1);
        g.limit = Some(g.limit.map_or(allowed, |l| l.min(allowed)));
        tracing::info!(key, limit = g.limit, "learned provider request limit per minute");
    }

    #[cfg(test)]
    pub fn reset(key: &str) {
        GATES.lock().remove(key);
    }
}
