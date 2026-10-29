//! MCP client: Model Context Protocol servers the user added in Settings
//! (local processes over stdio, remote endpoints over Streamable HTTP) lend
//! their tools to the agent. They are named `mcp__<server>__<tool>` and go
//! through the same risk policy, confirmations and audit as built-in tools.
//!
//! Servers start lazily. Tool lists are cached in SQLite, a server is
//! started on the first call and closed after [`IDLE`] without use, so an
//! idle JARVIS keeps no extra processes. Env values and HTTP headers (API
//! tokens) live in Credential Manager; SQLite only knows their names.

mod transport;

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::ai::types::ToolSpec;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::secrets;
use crate::tools::{Risk, ToolOutput};
use transport::Connection;

pub const PREFIX: &str = "mcp__";
const KV_SERVERS: &str = "mcp.servers";
const KV_TOOLS: &str = "mcp.tools";
const EVT: &str = "mcp:changed";
/// A server unused this long is shut down; the next call starts it again.
const IDLE: Duration = Duration::from_secs(600);
/// First start may download the package (`npx -y …`).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(120);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(180);
/// OpenAI-compatible APIs accept tool names up to 64 characters.
const MAX_NAME: usize = 64;
const MAX_DESCRIPTION: usize = 1000;
const MAX_TEXT: usize = 16_000;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    /// Short latin id, part of every tool name.
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub transport: TransportConfig,
    /// Names of the env variables / headers kept in Credential Manager.
    #[serde(default)]
    pub env_keys: Vec<String>,
    #[serde(default)]
    pub header_keys: Vec<String>,
    /// Tools the user switched off (original MCP names).
    #[serde(default)]
    pub disabled_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: String,
    },
    Http {
        url: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Secrets {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl Secrets {
    fn target(id: &str) -> String {
        format!("JARVIS/mcp/{id}")
    }
    fn load(id: &str) -> Secrets {
        secrets::get(&Self::target(id)).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    fn save(&self, id: &str) -> AppResult<()> {
        if self.env.is_empty() && self.headers.is_empty() {
            return secrets::delete(&Self::target(id));
        }
        let text = serde_json::to_string(self).map_err(|e| AppError::internal(e.to_string()))?;
        // Credential Manager holds at most 2560 bytes per entry.
        if text.len() > 2500 {
            return Err(AppError::invalid("Слишком длинные значения переменных и заголовков (больше 2,5 КБ)"));
        }
        secrets::set(&Self::target(id), &text)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Annotations {
    pub title: Option<String>,
    pub read_only_hint: Option<bool>,
    pub destructive_hint: Option<bool>,
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// A tool as the server describes it (`tools/list`), cached between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDef {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "empty_schema")]
    pub input_schema: Value,
    #[serde(default)]
    pub annotations: Annotations,
}

impl ToolDef {
    /// Annotations come from the server and are hints: a tool that does not
    /// say it only reads is confirmed once per conversation, an explicitly
    /// destructive one every time.
    pub fn risk(&self) -> Risk {
        if self.annotations.read_only_hint == Some(true) {
            Risk::Low
        } else if self.annotations.destructive_hint == Some(true) {
            Risk::High
        } else {
            Risk::Moderate
        }
    }
    fn label(&self) -> &str {
        self.annotations.title.as_deref().or(self.title.as_deref()).filter(|t| !t.trim().is_empty()).unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Switched off by the user.
    Off,
    /// Not running; tools known from the cache.
    Idle,
    Connecting,
    Running,
    Failed,
}

struct Server {
    config: ServerConfig,
    tools: RwLock<Vec<ToolDef>>,
    state: Mutex<(State, Option<String>, String)>,
    /// Serialises starting: concurrent calls wait for one connection.
    conn: tokio::sync::Mutex<Option<Arc<Connection>>>,
    last_used: Mutex<Instant>,
}

impl Server {
    fn new(config: ServerConfig, tools: Vec<ToolDef>) -> Arc<Server> {
        let state = if config.enabled { State::Idle } else { State::Off };
        Arc::new(Server {
            config,
            tools: RwLock::new(tools),
            state: Mutex::new((state, None, String::new())),
            conn: tokio::sync::Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub name: String,
    pub title: String,
    pub description: String,
    pub risk: &'static str,
    pub enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    pub config: ServerConfig,
    pub state: State,
    pub error: Option<String>,
    pub server_info: String,
    pub tools: Vec<ToolView>,
}

/// Risk, title and concurrency of one exposed MCP tool.
pub struct Meta {
    pub risk: Risk,
    pub title: String,
    pub parallel_safe: bool,
}

pub struct Hub {
    app: AppHandle,
    servers: RwLock<BTreeMap<String, Arc<Server>>>,
    /// Ids of servers whose tool list changed (read by [`Hub::spawn_background`]).
    reloads: tokio::sync::mpsc::UnboundedSender<String>,
    reload_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<String>>>,
}

/// Latin letters, digits, `_` and `-` only (the tool-name alphabet).
fn slug(s: &str, max: usize) -> String {
    let mut out: String = s
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '_' })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_');
    out.chars().take(max).collect::<String>().trim_end_matches('_').to_string()
}

/// `mcp__<server>__<tool>` within 64 characters, unique within `taken`.
fn exposed_name(server: &str, tool: &str, taken: &mut HashSet<String>) -> String {
    let base = format!("{PREFIX}{server}__");
    let room = MAX_NAME.saturating_sub(base.len());
    let t: String = tool.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let mut name = format!("{base}{}", t.chars().take(room).collect::<String>());
    let mut n = 2;
    while taken.contains(&name) {
        let suffix = format!("_{n}");
        name = format!("{base}{}{suffix}", t.chars().take(room.saturating_sub(suffix.len())).collect::<String>());
        n += 1;
    }
    taken.insert(name.clone());
    name
}

/// Providers reject some JSON-schema keywords at the top level and need an object.
fn clean_schema(schema: &Value) -> Value {
    let mut s = match schema {
        Value::Object(_) => schema.clone(),
        _ => empty_schema(),
    };
    if let Some(o) = s.as_object_mut() {
        o.remove("$schema");
        o.insert("type".into(), json!("object"));
        o.entry("properties").or_insert_with(|| json!({}));
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}… [обрезано]", s.chars().take(max).collect::<String>())
}

/// `tools/call` result → what the agent and the UI see.
fn to_output(server: &str, tool: &str, result: &Value) -> ToolOutput {
    let mut texts = Vec::new();
    let mut images = Vec::new();
    for item in result.get("content").and_then(Value::as_array).into_iter().flatten() {
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => texts.push(item.get("text").and_then(Value::as_str).unwrap_or("").to_string()),
            "image" => {
                let (mime, data) = (item.get("mimeType").and_then(Value::as_str), item.get("data").and_then(Value::as_str));
                if let (Some(m), Some(d)) = (mime, data) {
                    images.push(format!("data:{m};base64,{d}"));
                }
            }
            "resource" => {
                let r = item.get("resource");
                let uri = r.and_then(|r| r.get("uri")).and_then(Value::as_str).unwrap_or("");
                match r.and_then(|r| r.get("text")).and_then(Value::as_str) {
                    Some(t) => texts.push(format!("[{uri}]\n{t}")),
                    None => texts.push(format!("[двоичный ресурс {uri}]")),
                }
            }
            "resource_link" => {
                let uri = item.get("uri").and_then(Value::as_str).unwrap_or("");
                let name = item.get("name").and_then(Value::as_str).unwrap_or(uri);
                texts.push(format!("[ссылка: {name} — {uri}]"));
            }
            "audio" => texts.push("[аудио — не поддерживается]".into()),
            _ => {}
        }
    }
    let text = truncate(&texts.join("\n\n"), MAX_TEXT);
    let failed = result.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let mut data = json!({ "server": server, "tool": tool, "text": text });
    if let Some(s) = result.get("structuredContent") {
        data["structured"] = s.clone();
    }
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let summary = if failed {
        format!("{server}: ошибка — {}", truncate(first, 140))
    } else if first.is_empty() {
        format!("{server}: готово")
    } else {
        format!("{server}: {}", truncate(first, 140))
    };
    let mut out = if failed { ToolOutput::fail(summary, data) } else { ToolOutput::ok(summary, data) };
    out.images = images;
    out
}

/// The `mcpServers` object of Claude Desktop / Cursor / VS Code configs
/// (`servers` in VS Code), or one server given bare.
pub fn parse_import(text: &str) -> AppResult<Vec<(ServerConfig, Secrets)>> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| AppError::invalid(format!("Это не JSON: {e}")))?;
    let map = v
        .get("mcpServers")
        .or_else(|| v.get("servers"))
        .or_else(|| v.get("mcp").and_then(|m| m.get("servers")))
        .and_then(Value::as_object)
        .cloned()
        .or_else(|| (v.get("command").is_some() || v.get("url").is_some()).then(|| {
            let mut m = serde_json::Map::new();
            m.insert("server".into(), v.clone());
            m
        }))
        .or_else(|| v.as_object().filter(|o| o.values().all(|s| s.get("command").is_some() || s.get("url").is_some())).cloned())
        .ok_or_else(|| AppError::invalid("Не нашёл серверов: ожидается объект «mcpServers»"))?;
    let strings = |v: Option<&Value>| -> BTreeMap<String, String> {
        v.and_then(Value::as_object)
            .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for (key, s) in map {
        let transport = if let Some(url) = s.get("url").or_else(|| s.get("serverUrl")).and_then(Value::as_str) {
            if s.get("type").and_then(Value::as_str) == Some("sse") {
                return Err(AppError::unsupported(format!("«{key}»: старый транспорт SSE не поддерживается — нужен адрес Streamable HTTP")));
            }
            TransportConfig::Http { url: url.to_string() }
        } else if let Some(cmd) = s.get("command").and_then(Value::as_str) {
            let args = s.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
            let cwd = s.get("cwd").and_then(Value::as_str).unwrap_or("").to_string();
            TransportConfig::Stdio { command: cmd.to_string(), args, cwd }
        } else {
            return Err(AppError::invalid(format!("«{key}»: нет ни command, ни url")));
        };
        let secrets = Secrets { env: strings(s.get("env")), headers: strings(s.get("headers")) };
        let config = ServerConfig {
            id: slug(&key, 24),
            name: key.clone(),
            enabled: !s.get("disabled").and_then(Value::as_bool).unwrap_or(false),
            transport,
            env_keys: secrets.env.keys().cloned().collect(),
            header_keys: secrets.headers.keys().cloned().collect(),
            disabled_tools: Vec::new(),
        };
        out.push((config, secrets));
    }
    if out.is_empty() {
        return Err(AppError::invalid("Список серверов пуст"));
    }
    Ok(out)
}

impl Hub {
    pub fn new(app: AppHandle) -> Arc<Hub> {
        let store = app.state::<AppState>().store.clone();
        let configs: Vec<ServerConfig> = store.kv_get(KV_SERVERS).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let mut cache: BTreeMap<String, Vec<ToolDef>> = store.kv_get(KV_TOOLS).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let servers = configs.into_iter().map(|c| (c.id.clone(), Server::new(c.clone(), cache.remove(&c.id).unwrap_or_default()))).collect();
        let (reloads, rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(Hub { app, servers: RwLock::new(servers), reloads, reload_rx: Mutex::new(Some(rx)) })
    }

    fn store(&self) -> Arc<crate::store::Store> {
        self.app.state::<AppState>().store.clone()
    }

    fn persist(&self) -> AppResult<()> {
        let servers = self.servers.read();
        let configs: Vec<&ServerConfig> = servers.values().map(|s| &s.config).collect();
        let tools: BTreeMap<&String, Vec<ToolDef>> = servers.iter().map(|(id, s)| (id, s.tools.read().clone())).collect();
        let store = self.store();
        store.kv_set(KV_SERVERS, &serde_json::to_string(&configs).unwrap_or_else(|_| "[]".into()))?;
        store.kv_set(KV_TOOLS, &serde_json::to_string(&tools).unwrap_or_else(|_| "{}".into()))
    }

    fn changed(&self) {
        let _ = self.app.emit(EVT, ());
    }

    fn set_state(&self, server: &Server, state: State, error: Option<String>, info: Option<String>) {
        let mut s = server.state.lock();
        s.0 = state;
        s.1 = error;
        if let Some(i) = info {
            s.2 = i;
        }
        drop(s);
        self.changed();
    }

    /// Exposed name → (server, tool) for every enabled tool of every enabled server.
    fn exposed(&self) -> Vec<(String, Arc<Server>, ToolDef)> {
        let mut taken = HashSet::new();
        let mut out = Vec::new();
        for s in self.servers.read().values() {
            if !s.config.enabled {
                continue;
            }
            for t in s.tools.read().iter() {
                if s.config.disabled_tools.contains(&t.name) {
                    continue;
                }
                out.push((exposed_name(&s.config.id, &t.name, &mut taken), s.clone(), t.clone()));
            }
        }
        out
    }

    fn find(&self, name: &str) -> Option<(Arc<Server>, ToolDef)> {
        if !name.starts_with(PREFIX) {
            return None;
        }
        self.exposed().into_iter().find(|(n, ..)| n == name).map(|(_, s, t)| (s, t))
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.exposed()
            .into_iter()
            .map(|(name, s, t)| ToolSpec {
                name,
                description: truncate(&format!("[MCP-сервер «{}»] {}", s.config.name, t.description.trim()), MAX_DESCRIPTION),
                parameters: clean_schema(&t.input_schema),
            })
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.exposed().into_iter().map(|(n, ..)| n).collect()
    }

    pub fn describe(&self, name: &str) -> Option<Meta> {
        let (s, t) = self.find(name)?;
        let risk = t.risk();
        Some(Meta { risk, title: format!("{} · {}", s.config.name, t.label()), parallel_safe: risk == Risk::Low })
    }

    /// The live connection, starting the server if it is not running.
    async fn connection(self: &Arc<Self>, server: &Arc<Server>, cancel: &CancellationToken) -> AppResult<Arc<Connection>> {
        let mut slot = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            g = server.conn.lock() => g,
        };
        if let Some(c) = slot.as_ref().filter(|c| c.is_alive()) {
            return Ok(c.clone());
        }
        *slot = None;
        self.set_state(server, State::Connecting, None, None);
        let secrets = Secrets::load(&server.config.id);
        let on_change: transport::OnChange = {
            // By id: switching a tool off replaces the `Server` value.
            let (tx, id) = (self.reloads.clone(), server.config.id.clone());
            Arc::new(move || {
                let _ = tx.send(id.clone());
            })
        };
        let opened = tokio::select! {
            _ = cancel.cancelled() => Err(AppError::Cancelled),
            r = Connection::open(&server.config, &secrets, CONNECT_TIMEOUT, on_change) => r,
        };
        match opened {
            Ok(c) => {
                *slot = Some(c.clone());
                *server.last_used.lock() = Instant::now();
                self.set_state(server, State::Running, None, Some(c.server_info.clone()));
                Ok(c)
            }
            Err(e) => {
                let state = if matches!(e, AppError::Cancelled) { State::Idle } else { State::Failed };
                self.set_state(server, state, Some(e.to_string()), None);
                Err(e)
            }
        }
    }

    async fn reload_tools(self: &Arc<Self>, server: &Arc<Server>) -> AppResult<usize> {
        let conn = self.connection(server, &CancellationToken::new()).await?;
        let raw = conn.list_tools(LIST_TIMEOUT).await.map_err(|e| conn.explain(e))?;
        let tools: Vec<ToolDef> = raw.into_iter().filter_map(|t| serde_json::from_value(t).ok()).collect();
        let n = tools.len();
        *server.tools.write() = tools;
        self.persist()?;
        self.changed();
        Ok(n)
    }

    pub async fn call(self: &Arc<Self>, name: &str, args: Value, cancel: &CancellationToken) -> AppResult<ToolOutput> {
        let (server, tool) = self.find(name).ok_or_else(|| AppError::not_found(format!("Инструмента «{name}» нет")))?;
        let params = json!({ "name": tool.name, "arguments": if args.is_object() { args } else { json!({}) } });
        // One retry when the server died or forgot the session since last time.
        for attempt in 0..2 {
            let conn = self.connection(&server, cancel).await?;
            *server.last_used.lock() = Instant::now();
            let res = conn.request("tools/call", params.clone(), CALL_TIMEOUT, cancel).await;
            *server.last_used.lock() = Instant::now();
            match res {
                Ok(v) => return Ok(to_output(&server.config.name, tool.label(), &v)),
                Err(e) if attempt == 0 && !conn.is_alive() && !matches!(e, AppError::Cancelled) => {
                    tracing::warn!(server = %server.config.id, "mcp connection lost, restarting: {e}");
                }
                Err(e) => return Err(conn.explain(e)),
            }
        }
        unreachable!("the loop returns on its second attempt")
    }

    pub fn views(&self) -> Vec<ServerView> {
        self.servers
            .read()
            .values()
            .map(|s| {
                let (state, error, info) = s.state.lock().clone();
                ServerView {
                    config: s.config.clone(),
                    state,
                    error,
                    server_info: info,
                    tools: s
                        .tools
                        .read()
                        .iter()
                        .map(|t| ToolView {
                            name: t.name.clone(),
                            title: t.label().to_string(),
                            description: truncate(t.description.trim(), 240),
                            risk: t.risk().as_str(),
                            enabled: !s.config.disabled_tools.contains(&t.name),
                        })
                        .collect(),
                }
            })
            .collect()
    }

    /// Add or replace a server. `secrets: None` keeps the stored ones.
    pub async fn upsert(self: &Arc<Self>, mut config: ServerConfig, secrets: Option<Secrets>, previous_id: Option<String>) -> AppResult<()> {
        config.name = config.name.trim().to_string();
        if config.name.is_empty() {
            return Err(AppError::invalid("Укажите название сервера"));
        }
        config.id = slug(if config.id.trim().is_empty() { &config.name } else { &config.id }, 24);
        if config.id.is_empty() {
            config.id = format!("server{}", self.servers.read().len() + 1);
        }
        match &mut config.transport {
            TransportConfig::Stdio { command, .. } if command.trim().is_empty() => return Err(AppError::invalid("Укажите команду запуска")),
            TransportConfig::Http { url } if url.trim().is_empty() => return Err(AppError::invalid("Укажите адрес сервера")),
            TransportConfig::Stdio { command, .. } => *command = command.trim().to_string(),
            TransportConfig::Http { url } => *url = url.trim().to_string(),
        }
        let replacing = previous_id.filter(|p| self.servers.read().contains_key(p));
        if replacing.as_deref() != Some(config.id.as_str()) && self.servers.read().contains_key(&config.id) {
            return Err(AppError::invalid(format!("Сервер с идентификатором «{}» уже есть", config.id)));
        }
        let secrets = match secrets {
            Some(s) => s,
            None => replacing.as_deref().map(Secrets::load).unwrap_or_default(),
        };
        config.env_keys = secrets.env.keys().cloned().collect();
        config.header_keys = secrets.headers.keys().cloned().collect();
        secrets.save(&config.id)?;
        let tools = replacing.as_ref().and_then(|p| self.servers.read().get(p).map(|s| s.tools.read().clone())).unwrap_or_default();
        if let Some(p) = &replacing {
            self.servers.write().remove(p);
            if p != &config.id {
                let _ = secrets::delete(&Secrets::target(p));
            }
        }
        let server = Server::new(config.clone(), tools);
        self.servers.write().insert(config.id.clone(), server.clone());
        self.persist()?;
        self.changed();
        if config.enabled {
            self.probe(&server).await;
        }
        Ok(())
    }

    /// Start a server to learn its tools; leaves it running until idle.
    async fn probe(self: &Arc<Self>, server: &Arc<Server>) {
        if let Err(e) = self.reload_tools(server).await {
            tracing::warn!(server = %server.config.id, "mcp probe failed: {e}");
        }
    }

    pub async fn refresh(self: &Arc<Self>, id: &str) -> AppResult<usize> {
        let server = self.servers.read().get(id).cloned().ok_or_else(|| AppError::not_found("Сервер не найден"))?;
        *server.conn.lock().await = None;
        self.reload_tools(&server).await
    }

    pub async fn import(self: &Arc<Self>, text: &str) -> AppResult<Vec<String>> {
        let parsed = parse_import(text)?;
        let mut added = Vec::new();
        for (mut config, secrets) in parsed {
            let base = config.id.clone();
            let mut n = 2;
            while self.servers.read().contains_key(&config.id) {
                config.id = format!("{base}{n}");
                n += 1;
            }
            added.push(config.name.clone());
            self.upsert(config, Some(secrets), None).await?;
        }
        Ok(added)
    }

    pub fn remove(&self, id: &str) -> AppResult<()> {
        self.servers.write().remove(id);
        let _ = secrets::delete(&Secrets::target(id));
        self.persist()?;
        self.changed();
        Ok(())
    }

    fn edit(&self, id: &str, f: impl FnOnce(&mut ServerConfig)) -> AppResult<Arc<Server>> {
        let old = self.servers.read().get(id).cloned().ok_or_else(|| AppError::not_found("Сервер не найден"))?;
        let mut config = old.config.clone();
        f(&mut config);
        let server = Server::new(config, old.tools.read().clone());
        // Keep a running server: only the switches changed.
        *server.state.lock() = old.state.lock().clone();
        *server.last_used.lock() = *old.last_used.lock();
        if let (Ok(mut new), Ok(mut old)) = (server.conn.try_lock(), old.conn.try_lock()) {
            *new = old.take();
        }
        self.servers.write().insert(id.to_string(), server.clone());
        self.persist()?;
        self.changed();
        Ok(server)
    }

    pub async fn set_enabled(self: &Arc<Self>, id: &str, enabled: bool) -> AppResult<()> {
        let server = self.edit(id, |c| c.enabled = enabled)?;
        if enabled {
            if server.tools.read().is_empty() {
                self.probe(&server).await;
            } else {
                self.set_state(&server, State::Idle, None, None);
            }
        } else {
            *server.conn.lock().await = None;
            self.set_state(&server, State::Off, None, None);
        }
        Ok(())
    }

    pub fn set_tool_enabled(&self, id: &str, tool: &str, enabled: bool) -> AppResult<()> {
        self.edit(id, |c| {
            c.disabled_tools.retain(|t| t != tool);
            if !enabled {
                c.disabled_tools.push(tool.to_string());
            }
        })
        .map(|_| ())
    }

    /// Shut down servers nobody used for [`IDLE`]; reload tool lists that changed.
    pub fn spawn_background(self: &Arc<Self>) {
        if let Some(mut rx) = self.reload_rx.lock().take() {
            // Enabled servers whose tools were never read: learn them after start-up.
            for s in self.servers.read().values().filter(|s| s.config.enabled && s.tools.read().is_empty()) {
                let _ = self.reloads.send(s.config.id.clone());
            }
            let hub = self.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(8)).await;
                while let Some(id) = rx.recv().await {
                    let Some(server) = hub.servers.read().get(&id).cloned() else { continue };
                    if let Err(e) = hub.reload_tools(&server).await {
                        tracing::warn!(server = %id, "mcp tools reload failed: {e}");
                    }
                }
            });
        }
        let hub = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let servers: Vec<Arc<Server>> = hub.servers.read().values().cloned().collect();
                for s in servers {
                    if s.last_used.lock().elapsed() < IDLE {
                        continue;
                    }
                    let Ok(mut slot) = s.conn.try_lock() else { continue };
                    if slot.take().is_some() {
                        tracing::info!(server = %s.config.id, "mcp server idle, stopped");
                        drop(slot);
                        let state = if s.config.enabled { State::Idle } else { State::Off };
                        hub.set_state(&s, state, None, None);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_fit_the_tool_alphabet_and_length() {
        let mut taken = HashSet::new();
        assert_eq!(exposed_name("github", "create_issue", &mut taken), "mcp__github__create_issue");
        let long = exposed_name("github", &"x".repeat(100), &mut taken);
        assert_eq!(long.len(), MAX_NAME);
        let dup = exposed_name("github", &"x".repeat(100), &mut taken);
        assert!(dup.len() <= MAX_NAME && dup != long && dup.ends_with("_2"));
        assert_eq!(exposed_name("fs", "read.file", &mut taken), "mcp__fs__read_file");
        assert_eq!(slug("My GitHub (work)", 24), "my_github_work");
        assert_eq!(slug("Файлы", 24), "");
    }

    #[test]
    fn risk_follows_annotations() {
        let tool = |ro: Option<bool>, d: Option<bool>| ToolDef {
            name: "t".into(),
            title: None,
            description: String::new(),
            input_schema: empty_schema(),
            annotations: Annotations { title: None, read_only_hint: ro, destructive_hint: d },
        };
        assert_eq!(tool(Some(true), None).risk(), Risk::Low);
        assert_eq!(tool(None, None).risk(), Risk::Moderate);
        assert_eq!(tool(Some(false), Some(false)).risk(), Risk::Moderate);
        assert_eq!(tool(None, Some(true)).risk(), Risk::High);
    }

    #[test]
    fn imports_claude_desktop_config() {
        let text = r#"{ "mcpServers": {
            "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\me"] },
            "github": { "command": "docker", "args": ["run"], "env": { "GITHUB_TOKEN": "secret" } },
            "remote": { "type": "http", "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer x" } }
        } }"#;
        let servers = parse_import(text).unwrap();
        assert_eq!(servers.len(), 3);
        let (fs, _) = servers.iter().find(|(c, _)| c.id == "filesystem").unwrap();
        assert!(matches!(&fs.transport, TransportConfig::Stdio { command, args, .. } if command == "npx" && args.len() == 3));
        let (gh, gs) = servers.iter().find(|(c, _)| c.id == "github").unwrap();
        assert_eq!(gh.env_keys, vec!["GITHUB_TOKEN".to_string()]);
        assert_eq!(gs.env["GITHUB_TOKEN"], "secret");
        let (remote, rs) = servers.iter().find(|(c, _)| c.id == "remote").unwrap();
        assert!(matches!(&remote.transport, TransportConfig::Http { url } if url == "https://example.com/mcp"));
        assert_eq!(rs.headers["Authorization"], "Bearer x");
        assert!(parse_import(r#"{"mcpServers":{"old":{"type":"sse","url":"http://x/sse"}}}"#).is_err());
        assert_eq!(parse_import(r#"{"command":"uvx","args":["mcp-server-time"]}"#).unwrap().len(), 1);
    }

    #[test]
    fn converts_call_results() {
        let r = json!({ "content": [
            { "type": "text", "text": "Found 2 issues\nmore" },
            { "type": "image", "mimeType": "image/png", "data": "AAAA" }
        ], "structuredContent": { "count": 2 } });
        let out = to_output("GitHub", "search", &r);
        assert!(out.ok);
        assert_eq!(out.summary, "GitHub: Found 2 issues");
        assert_eq!(out.images, vec!["data:image/png;base64,AAAA".to_string()]);
        assert_eq!(out.data["structured"]["count"], 2);
        let err = to_output("GitHub", "search", &json!({ "content": [{ "type": "text", "text": "rate limited" }], "isError": true }));
        assert!(!err.ok);
        assert!(err.summary.contains("ошибка"));
    }

    #[test]
    fn schemas_are_cleaned_for_providers() {
        let s = clean_schema(&json!({ "$schema": "http://json-schema.org/draft-07/schema#", "type": "object" }));
        assert!(s.get("$schema").is_none());
        assert_eq!(s["properties"], json!({}));
        assert_eq!(clean_schema(&Value::Null)["type"], "object");
    }
}
