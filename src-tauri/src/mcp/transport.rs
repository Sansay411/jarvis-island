//! JSON-RPC 2.0 sessions with MCP servers over the two standard transports:
//! stdio (a child process, newline-delimited messages) and Streamable HTTP
//! (POST per message; the reply is JSON or an SSE stream). Both perform the
//! `initialize` handshake on open and answer the few requests a server may
//! send us (`ping`, `roots/list`).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio as PipeMode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::{Secrets, ServerConfig, TransportConfig};
use crate::error::{AppError, AppResult};
use crate::platform::supervisor::{which, Job};

/// The revision we speak; servers answer with the one they support and the
/// parts we use (tools, ping) are the same in all of them.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STDERR_LINES: usize = 20;

type Reply = Result<Value, String>;
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Reply>>>>;
/// Called when the server says its tool list changed.
pub type OnChange = Arc<dyn Fn() + Send + Sync>;

pub struct Connection {
    kind: Kind,
    next_id: AtomicU64,
    /// «name version» from the server's `initialize` reply.
    pub server_info: String,
}

enum Kind {
    Stdio(StdioConn),
    Http(HttpConn),
}

impl Connection {
    pub async fn open(cfg: &ServerConfig, secrets: &Secrets, timeout: Duration, on_change: OnChange) -> AppResult<Arc<Connection>> {
        let kind = match &cfg.transport {
            TransportConfig::Stdio { command, args, cwd } => Kind::Stdio(StdioConn::spawn(command, args, cwd, secrets, on_change)?),
            TransportConfig::Http { url } => Kind::Http(HttpConn::new(url, secrets, on_change)?),
        };
        let mut conn = Connection { kind, next_id: AtomicU64::new(1), server_info: String::new() };
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "jarvis", "title": "JARVIS", "version": env!("CARGO_PKG_VERSION") },
        });
        let init = conn.request("initialize", params, timeout, &CancellationToken::new()).await.map_err(|e| conn.explain(e))?;
        let version = init.get("protocolVersion").and_then(Value::as_str).unwrap_or("").to_string();
        if let Kind::Http(h) = &conn.kind {
            *h.protocol.lock() = Some(version.clone());
        }
        let info = init.get("serverInfo");
        let name = info.and_then(|i| i.get("name")).and_then(Value::as_str).unwrap_or("");
        let ver = info.and_then(|i| i.get("version")).and_then(Value::as_str).unwrap_or("");
        conn.server_info = format!("{name} {ver}").trim().to_string();
        tracing::info!(server = %cfg.id, info = %conn.server_info, protocol = %version, "mcp initialized");
        conn.notify("notifications/initialized", json!({})).await?;
        Ok(Arc::new(conn))
    }

    pub fn is_alive(&self) -> bool {
        match &self.kind {
            Kind::Stdio(s) => s.alive.load(Ordering::Relaxed),
            Kind::Http(h) => !h.expired.load(Ordering::Relaxed),
        }
    }

    /// Adds what the server printed to stderr: the usual reason it failed.
    pub fn explain(&self, e: AppError) -> AppError {
        let Kind::Stdio(s) = &self.kind else { return e };
        let tail: Vec<String> = s.stderr.lock().iter().cloned().collect();
        if tail.is_empty() || matches!(e, AppError::Cancelled) {
            return e;
        }
        let tail = tail.join("\n");
        let tail: String = tail.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
        AppError::Provider(format!("{e}. Сервер сообщил:\n{tail}"))
    }

    pub async fn request(&self, method: &str, params: Value, timeout: Duration, cancel: &CancellationToken) -> AppResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let fut = async {
            match &self.kind {
                Kind::Stdio(s) => s.request(id, &msg).await,
                Kind::Http(h) => h.request(id, &msg).await,
            }
        };
        let res = tokio::select! {
            _ = cancel.cancelled() => Err(AppError::Cancelled),
            r = tokio::time::timeout(timeout, fut) => r.unwrap_or_else(|_| {
                Err(AppError::Timeout(format!("MCP-сервер не ответил на {method} за {} с", timeout.as_secs())))
            }),
        };
        if let Kind::Stdio(s) = &self.kind {
            s.pending.lock().remove(&id);
        }
        if matches!(res, Err(AppError::Cancelled) | Err(AppError::Timeout(_))) {
            let _ = self.notify("notifications/cancelled", json!({ "requestId": id, "reason": "cancelled by the user" })).await;
        }
        res
    }

    pub async fn notify(&self, method: &str, params: Value) -> AppResult<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        match &self.kind {
            Kind::Stdio(s) => s.send(&msg).await,
            Kind::Http(h) => h.post(&msg).await.map(|_| ()),
        }
    }

    /// All pages of `tools/list`.
    pub async fn list_tools(&self, timeout: Duration) -> AppResult<Vec<Value>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, timeout, &CancellationToken::new()).await?;
            if let Some(list) = page.get("tools").and_then(Value::as_array) {
                tools.extend(list.iter().cloned());
            }
            cursor = page.get("nextCursor").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }
}

/// Replies to requests a server sends us. We advertise no client
/// capabilities, so only the always-allowed ones succeed.
fn answer(req: &Value) -> Value {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    match req.get("method").and_then(Value::as_str).unwrap_or("") {
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "roots/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "roots": [] } }),
        m => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("Method not found: {m}") } }),
    }
}

enum Incoming {
    Response(u64, Reply),
    Request(Value),
    Notification(String),
    Other,
}

fn classify(msg: Value) -> Incoming {
    let method = msg.get("method").and_then(Value::as_str).map(str::to_string);
    let id = msg.get("id");
    match (method, id) {
        (Some(_), Some(id)) if !id.is_null() => Incoming::Request(msg),
        (Some(m), _) => Incoming::Notification(m),
        (None, Some(id)) => {
            let Some(id) = id.as_u64() else { return Incoming::Other };
            if let Some(err) = msg.get("error") {
                let text = err.get("message").and_then(Value::as_str).unwrap_or("неизвестная ошибка").to_string();
                Incoming::Response(id, Err(text))
            } else {
                Incoming::Response(id, Ok(msg.get("result").cloned().unwrap_or(Value::Null)))
            }
        }
        _ => Incoming::Other,
    }
}

fn rpc_error(text: String) -> AppError {
    AppError::Provider(format!("MCP-сервер вернул ошибку: {text}"))
}

// ---------- stdio ----------

struct StdioConn {
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    alive: Arc<AtomicBool>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    // Dropping these kills the server and everything it started.
    _child: Child,
    _job: Option<Job>,
}

/// `npx`, `uvx` and friends are .cmd shims on Windows: resolve them on PATH
/// the way a shell would (Rust's spawn only appends .exe).
fn resolve(command: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(command);
    if p.is_absolute() || command.contains(['/', '\\']) {
        return if p.is_file() { Ok(p) } else { Err(AppError::not_found(format!("Файл «{command}» не найден"))) };
    }
    // Next to `npx.cmd` Node ships an extension-less `npx` shell script that
    // Windows cannot start: without an extension, look for the shims first.
    let found = if p.extension().is_none() {
        [".exe", ".cmd", ".bat", ".com"].iter().find_map(|ext| which(&format!("{command}{ext}"))).or_else(|| which(command))
    } else {
        which(command)
    };
    found.ok_or_else(|| {
        let hint = match command.to_lowercase().as_str() {
            "npx" | "node" | "npm" => " Установите Node.js (nodejs.org) и перезапустите Джарвиса.",
            "uvx" | "uv" => " Установите uv (docs.astral.sh/uv) и перезапустите Джарвиса.",
            "docker" => " Установите Docker Desktop.",
            _ => "",
        };
        AppError::not_found(format!("Команда «{command}» не найдена в PATH.{hint}"))
    })
}

/// `%USERPROFILE%\Documents` → the real path, as cmd.exe would; unknown
/// variables stay as written.
fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 && after[..end].chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '(' || c == ')') => {
                out.push_str(&rest[..start]);
                match std::env::var(&after[..end]) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

impl StdioConn {
    fn spawn(command: &str, args: &[String], cwd: &str, secrets: &Secrets, on_change: OnChange) -> AppResult<StdioConn> {
        let program = resolve(&expand(command.trim()))?;
        let mut cmd = Command::new(&program);
        cmd.args(args.iter().map(|a| expand(a)))
            .stdin(PipeMode::piped())
            .stdout(PipeMode::piped())
            .stderr(PipeMode::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .kill_on_drop(true);
        let dir = if cwd.trim().is_empty() { dirs::home_dir() } else { Some(PathBuf::from(expand(cwd.trim()))) };
        if let Some(d) = dir.filter(|d| d.is_dir()) {
            cmd.current_dir(d);
        }
        for (k, v) in &secrets.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| AppError::platform(format!("Не удалось запустить «{command}»: {e}")))?;
        let job = Job::new().ok();
        if let Some(j) = &job {
            j.assign(&child);
        }
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            return Err(AppError::internal("Нет каналов ввода-вывода MCP-сервера"));
        };
        let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();

        {
            let tail = tail.clone();
            let name = command.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).split(b'\n');
                while let Ok(Some(raw)) = lines.next_segment().await {
                    let line = String::from_utf8_lossy(&raw).trim_end().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    tracing::debug!(server = %name, "mcp stderr: {}", line.chars().take(300).collect::<String>());
                    let mut t = tail.lock();
                    if t.len() == STDERR_LINES {
                        t.pop_front();
                    }
                    t.push_back(line.chars().take(300).collect());
                }
            });
        }
        {
            let (pending, alive, stdin) = (pending.clone(), alive.clone(), stdin.clone());
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).split(b'\n');
                while let Ok(Some(raw)) = lines.next_segment().await {
                    let Ok(msg) = serde_json::from_slice::<Value>(&raw) else {
                        // Servers sometimes log to stdout; that is not protocol.
                        continue;
                    };
                    for m in if let Value::Array(batch) = msg { batch } else { vec![msg] } {
                        match classify(m) {
                            Incoming::Response(id, reply) => {
                                if let Some(tx) = pending.lock().remove(&id) {
                                    let _ = tx.send(reply);
                                }
                            }
                            Incoming::Request(req) => {
                                let line = format!("{}\n", answer(&req));
                                let _ = stdin.lock().await.write_all(line.as_bytes()).await;
                            }
                            Incoming::Notification(m) if m == "notifications/tools/list_changed" => on_change(),
                            _ => {}
                        }
                    }
                }
                alive.store(false, Ordering::Relaxed);
                // Wake everyone still waiting: the server is gone.
                for (_, tx) in pending.lock().drain() {
                    let _ = tx.send(Err("сервер завершил работу".into()));
                }
            });
        }
        Ok(StdioConn { stdin, pending, alive, stderr: tail, _child: child, _job: job })
    }

    async fn send(&self, msg: &Value) -> AppResult<()> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err(AppError::Network("MCP-сервер завершил работу".into()));
        }
        let line = format!("{msg}\n");
        let mut w = self.stdin.lock().await;
        w.write_all(line.as_bytes()).await.map_err(|e| AppError::Network(format!("MCP-сервер не принимает данные: {e}")))?;
        w.flush().await.map_err(|e| AppError::Network(format!("MCP-сервер не принимает данные: {e}")))
    }

    async fn request(&self, id: u64, msg: &Value) -> AppResult<Value> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        self.send(msg).await?;
        match rx.await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(text)) => Err(rpc_error(text)),
            Err(_) => Err(AppError::Network("MCP-сервер завершил работу".into())),
        }
    }
}

// ---------- Streamable HTTP ----------

struct HttpConn {
    url: String,
    headers: Vec<(String, String)>,
    session: Mutex<Option<String>>,
    protocol: Mutex<Option<String>>,
    /// The server forgot our session (HTTP 404): reconnect.
    expired: AtomicBool,
    on_change: OnChange,
}

impl HttpConn {
    fn new(url: &str, secrets: &Secrets, on_change: OnChange) -> AppResult<HttpConn> {
        let parsed = url::Url::parse(url.trim()).map_err(|_| AppError::invalid(format!("Некорректный адрес MCP-сервера: {url}")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(AppError::invalid("Адрес MCP-сервера должен начинаться с http:// или https://"));
        }
        Ok(HttpConn {
            url: parsed.to_string(),
            headers: secrets.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            session: Mutex::new(None),
            protocol: Mutex::new(None),
            expired: AtomicBool::new(false),
            on_change,
        })
    }

    async fn post(&self, msg: &Value) -> AppResult<reqwest::Response> {
        let mut req = crate::ai::openai::http_client()
            .post(&self.url)
            .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
            .json(msg);
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        if let Some(s) = self.session.lock().clone() {
            req = req.header("Mcp-Session-Id", s);
        }
        if let Some(p) = self.protocol.lock().clone() {
            req = req.header("MCP-Protocol-Version", p);
        }
        let resp = req.send().await.map_err(|e| AppError::Network(format!("MCP-сервер недоступен: {e}")))?;
        if let Some(s) = resp.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            *self.session.lock() = Some(s.to_string());
        }
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body: String = resp.text().await.unwrap_or_default().chars().take(300).collect();
        Err(match status.as_u16() {
            401 | 403 => AppError::PermissionDenied(
                "Сервер требует авторизацию. Добавьте заголовок Authorization с токеном; вход через OAuth пока не поддерживается".into(),
            ),
            404 if self.session.lock().is_some() => {
                self.expired.store(true, Ordering::Relaxed);
                AppError::Network("MCP-сервер завершил сессию".into())
            }
            code => AppError::Network(format!("MCP-сервер ответил {code}: {}", body.trim())),
        })
    }

    async fn request(&self, id: u64, msg: &Value) -> AppResult<Value> {
        let resp = self.post(msg).await?;
        let sse = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/event-stream"));
        if !sse {
            let body: Value = resp.json().await.map_err(|e| AppError::Network(format!("Некорректный ответ MCP-сервера: {e}")))?;
            let msgs = if let Value::Array(b) = body { b } else { vec![body] };
            for m in msgs {
                if let Some(r) = self.handle(m, id).await {
                    return r;
                }
            }
            return Err(AppError::Network("MCP-сервер не прислал ответ на запрос".into()));
        }
        let mut stream = resp.bytes_stream().eventsource();
        while let Some(ev) = stream.next().await {
            let ev = ev.map_err(|e| AppError::Network(format!("Поток MCP-сервера прервался: {e}")))?;
            let Ok(m) = serde_json::from_str::<Value>(&ev.data) else { continue };
            if let Some(r) = self.handle(m, id).await {
                return r;
            }
        }
        Err(AppError::Network("MCP-сервер закрыл поток, не ответив".into()))
    }

    /// `Some` when `m` is the response to request `id`.
    async fn handle(&self, m: Value, id: u64) -> Option<AppResult<Value>> {
        match classify(m) {
            Incoming::Response(rid, reply) if rid == id => Some(reply.map_err(rpc_error)),
            Incoming::Request(req) => {
                let _ = self.post(&answer(&req)).await;
                None
            }
            Incoming::Notification(n) if n == "notifications/tools/list_changed" => {
                (self.on_change)();
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_messages() {
        assert!(matches!(classify(json!({"jsonrpc":"2.0","id":3,"result":{"x":1}})), Incoming::Response(3, Ok(_))));
        assert!(matches!(classify(json!({"jsonrpc":"2.0","id":4,"error":{"code":-1,"message":"no"}})), Incoming::Response(4, Err(ref t)) if t == "no"));
        assert!(matches!(classify(json!({"jsonrpc":"2.0","id":"a","method":"ping"})), Incoming::Request(_)));
        assert!(matches!(classify(json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})), Incoming::Notification(_)));
    }

    async fn exercise(cfg: ServerConfig) {
        let conn = Connection::open(&cfg, &Secrets::default(), Duration::from_secs(180), Arc::new(|| {})).await.expect("initialize");
        assert!(!conn.server_info.is_empty());
        let tools = conn.list_tools(Duration::from_secs(30)).await.expect("tools/list");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        println!("{} → {} tools: {names:?}", conn.server_info, names.len());
        assert!(names.contains(&"echo"));
        let r = conn
            .request("tools/call", json!({ "name": "echo", "arguments": { "message": "привет, Джарвис" } }), Duration::from_secs(30), &CancellationToken::new())
            .await
            .expect("tools/call");
        println!("echo → {r}");
        assert!(r.to_string().contains("привет, Джарвис"));
        let bad = conn.request("tools/call", json!({ "name": "no_such_tool", "arguments": {} }), Duration::from_secs(30), &CancellationToken::new()).await;
        println!("unknown tool → {bad:?}");
    }

    /// `cargo test --lib live_stdio -- --ignored --nocapture` (needs Node.js and the network).
    #[tokio::test]
    #[ignore]
    async fn live_stdio() {
        exercise(ServerConfig {
            id: "everything".into(),
            name: "everything".into(),
            enabled: true,
            transport: TransportConfig::Stdio { command: "npx".into(), args: vec!["-y".into(), "@modelcontextprotocol/server-everything".into()], cwd: String::new() },
            env_keys: vec![],
            header_keys: vec![],
            disabled_tools: vec![],
        })
        .await;
    }

    /// Start `npx @modelcontextprotocol/server-everything streamableHttp` first;
    /// set JARVIS_MCP_HTTP_URL (default http://localhost:3001/mcp).
    #[tokio::test]
    #[ignore]
    async fn live_http() {
        let url = std::env::var("JARVIS_MCP_HTTP_URL").unwrap_or_else(|_| "http://localhost:3001/mcp".into());
        exercise(ServerConfig {
            id: "everything".into(),
            name: "everything".into(),
            enabled: true,
            transport: TransportConfig::Http { url },
            env_keys: vec![],
            header_keys: vec![],
            disabled_tools: vec![],
        })
        .await;
    }

    #[test]
    fn expands_percent_variables() {
        std::env::set_var("JARVIS_MCP_TEST", r"C:\Users\me");
        assert_eq!(expand(r"%JARVIS_MCP_TEST%\Documents"), r"C:\Users\me\Documents");
        assert_eq!(expand("%NO_SUCH_VAR_XYZ%/a"), "%NO_SUCH_VAR_XYZ%/a");
        assert_eq!(expand("100% sure"), "100% sure");
        assert_eq!(expand("a%%b"), "a%%b");
    }

    #[test]
    fn answers_ping_and_rejects_unknown() {
        assert_eq!(answer(&json!({"id":7,"method":"ping"}))["result"], json!({}));
        assert_eq!(answer(&json!({"id":8,"method":"sampling/createMessage"}))["error"]["code"], json!(-32601));
    }
}
