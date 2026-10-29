//! Tool registry. Every capability the agent can use is a [`Tool`] with a
//! JSON-schema input, a risk level computed from the actual arguments, a
//! timeout, a human-readable title for the UI and an async handler that
//! returns a *verified* [`ToolOutput`]. Tools of the MCP servers the user
//! connected ([`crate::mcp`]) are listed and run through the same registry.

mod apps;
mod browser;
mod dev;
mod file_actions;
mod files;
mod memory;
mod reminders;
pub mod screen;
mod shell;
mod system;
mod web;

pub use files::image_data_url;
pub use screen::sensitive_text;
pub use dev::{claude_exe, codex_entry, vscode_exe, vscode_recent};

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

use crate::ai::types::ToolSpec;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Reversible, local, non-sensitive: runs without asking.
    Low,
    /// Changes user data or reads sensitive content: confirmation by default.
    Moderate,
    /// Destructive, externally visible or system-level: always confirmed.
    High,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Moderate => "moderate",
            Risk::High => "high",
        }
    }
}

/// Result returned to the model (as JSON) and summarised in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ToolOutput {
    pub ok: bool,
    /// One short Russian sentence describing the *verified* outcome.
    pub summary: String,
    /// Structured data for the model.
    pub data: Value,
    /// Images (data URLs) the model should see, e.g. screenshots.
    #[serde(skip)]
    pub images: Vec<String>,
}

impl ToolOutput {
    pub fn ok(summary: impl Into<String>, data: Value) -> Self {
        ToolOutput { ok: true, summary: summary.into(), data, images: Vec::new() }
    }
    pub fn fail(summary: impl Into<String>, data: Value) -> Self {
        ToolOutput { ok: false, summary: summary.into(), data, images: Vec::new() }
    }
}

#[derive(Clone)]
pub struct ToolCtx {
    pub app: AppHandle,
    pub cancel: CancellationToken,
    pub task_id: String,
    /// Paths the user attached to the current conversation turn.
    pub attachments: Arc<Vec<String>>,
    /// Streams progress lines (e.g. process output) to the UI.
    pub progress: crate::platform::supervisor::LineSink,
}

impl ToolCtx {
    pub fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }
}

pub type ToolFuture = Pin<Box<dyn Future<Output = AppResult<ToolOutput>> + Send>>;
pub type Handler = Arc<dyn Fn(ToolCtx, Value) -> ToolFuture + Send + Sync>;

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub risk: fn(&Value) -> Risk,
    pub title: fn(&Value) -> String,
    pub timeout: Duration,
    /// Safe to run concurrently with other parallel-safe calls.
    pub parallel_safe: bool,
    pub handler: Handler,
}

pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(ToolCtx, Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = AppResult<ToolOutput>> + Send + 'static,
{
    Arc::new(move |ctx, args| Box::pin(f(ctx, args)))
}

pub fn low(_: &Value) -> Risk {
    Risk::Low
}

/// Parse tool arguments into a typed struct with a model-readable error.
pub fn parse<T: serde::de::DeserializeOwned>(args: Value) -> AppResult<T> {
    serde_json::from_value(args).map_err(|e| AppError::invalid(format!("Некорректные аргументы инструмента: {e}")))
}

pub fn str_arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("")
}

pub struct Registry {
    tools: BTreeMap<&'static str, Tool>,
    mcp: Arc<crate::mcp::Hub>,
}

/// What the agent needs to decide on a call before running it.
pub struct ToolMeta {
    pub name: String,
    pub risk: Risk,
    pub title: String,
    pub parallel_safe: bool,
}

impl Registry {
    pub fn new(mcp: Arc<crate::mcp::Hub>) -> Self {
        let mut tools = BTreeMap::new();
        for t in system::tools()
            .into_iter()
            .chain(apps::tools())
            .chain(files::tools())
            .chain(shell::tools())
            .chain(memory::tools())
            .chain(screen::tools())
            .chain(dev::tools())
            .chain(browser::tools())
            .chain(web::tools())
            .chain(file_actions::tools())
            .chain(reminders::tools())
        {
            tools.insert(t.name, t);
        }
        Registry { tools, mcp }
    }

    /// Risk (assessed from the arguments), title and concurrency of a tool.
    pub fn describe(&self, name: &str, args: &Value) -> Option<ToolMeta> {
        if let Some(t) = self.tools.get(name) {
            return Some(ToolMeta { name: t.name.to_string(), risk: (t.risk)(args), title: (t.title)(args), parallel_safe: t.parallel_safe });
        }
        let m = self.mcp.describe(name)?;
        Some(ToolMeta { name: name.to_string(), risk: m.risk, title: m.title, parallel_safe: m.parallel_safe })
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self
            .tools
            .values()
            .map(|t| ToolSpec { name: t.name.to_string(), description: t.description.to_string(), parameters: t.schema.clone() })
            .collect();
        specs.extend(self.mcp.specs());
        specs
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().map(|n| n.to_string()).chain(self.mcp.names()).collect()
    }

    /// Run a tool with its timeout and the task's cancellation token.
    pub async fn run(&self, name: &str, ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
        if !self.tools.contains_key(name) && name.starts_with(crate::mcp::PREFIX) {
            // MCP calls keep their own start-up and call timeouts.
            return self.mcp.call(name, args, &ctx.cancel).await;
        }
        let tool = self.tools.get(name).ok_or_else(|| AppError::not_found(format!("Инструмент «{name}» не существует")))?;
        let fut = (tool.handler)(ctx.clone(), args);
        tokio::select! {
            _ = ctx.cancel.cancelled() => Err(AppError::Cancelled),
            r = tokio::time::timeout(tool.timeout, fut) => r.unwrap_or_else(|_| {
                Err(AppError::Timeout(format!("Инструмент не уложился в {} с", tool.timeout.as_secs())))
            }),
        }
    }
}

/// Convenience for building JSON schemas inline.
pub fn object(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}
