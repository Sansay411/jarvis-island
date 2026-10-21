//! Tool registry. Every capability the agent can use is a [`Tool`] with a
//! JSON-schema input, a risk level computed from the actual arguments, a
//! timeout, a human-readable title for the UI and an async handler that
//! returns a *verified* [`ToolOutput`]. The registry is provider-agnostic and
//! can be exposed through MCP-style adapters later (name + description +
//! input schema + structured output).

mod apps;
mod browser;
mod dev;
mod files;
mod memory;
mod screen;
mod shell;
mod system;

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
    /// Its summary fully answers a simple request, so a successful call can
    /// end the turn without another model round-trip.
    pub fast_path: bool,
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
}

impl Registry {
    pub fn new() -> Self {
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
        {
            tools.insert(t.name, t);
        }
        Registry { tools }
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|t| ToolSpec { name: t.name.to_string(), description: t.description.to_string(), parameters: t.schema.clone() })
            .collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    /// Run a tool with its timeout and the task's cancellation token.
    pub async fn run(&self, name: &str, ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
        let tool = self.get(name).ok_or_else(|| AppError::not_found(format!("Инструмент «{name}» не существует")))?;
        let fut = (tool.handler)(ctx.clone(), args);
        tokio::select! {
            _ = ctx.cancel.cancelled() => Err(AppError::Cancelled),
            r = tokio::time::timeout(tool.timeout, fut) => r.unwrap_or_else(|_| {
                Err(AppError::Timeout(format!("Инструмент не уложился в {} с", tool.timeout.as_secs())))
            }),
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience for building JSON schemas inline.
pub fn object(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}
