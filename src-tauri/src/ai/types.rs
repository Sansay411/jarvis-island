//! Provider-neutral chat types. Adapters translate these to each API's wire
//! format; history is persisted in this shape.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    /// `data:` URL (base64) of a JPEG/PNG image.
    Image { data_url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments exactly as the model produced them.
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        ChatMessage { role, parts: vec![Part::Text { text: text.into() }], tool_calls: Vec::new(), tool_call_id: None }
    }

    pub fn tool_result(call_id: &str, content: impl Into<String>) -> Self {
        ChatMessage {
            role: Role::Tool,
            parts: vec![Part::Text { text: content.into() }],
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.to_string()),
        }
    }

    /// Concatenated text parts.
    pub fn joined_text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                Part::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn has_images(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Image { .. }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Incremental output while a completion streams.
#[derive(Debug, Clone)]
pub enum StreamDelta {
    Text(String),
    /// A tool call has started (name known); arguments are still streaming.
    ToolCallStarted { index: usize, name: String },
    /// Waiting for the provider's rate limit before (re)sending, seconds.
    Waiting(u64),
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub message: ChatMessage,
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,
    /// Time to first streamed token, for latency diagnostics.
    pub first_token_ms: Option<u64>,
}
