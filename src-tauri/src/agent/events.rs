//! Events streamed to the island while the agent works.
//! Mirrored in src/lib/agent-types.ts.

use serde::Serialize;
use serde_json::Value;

pub const EVT_AGENT: &str = "agent:event";

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AgentEvent {
    TurnStarted { turn_id: String, conversation_id: String, user_text: String },
    Phase { turn_id: String, phase: &'static str },
    TextDelta { turn_id: String, text: String },
    ToolStarted { turn_id: String, call_id: String, tool: String, title: String, args: Value },
    ToolProgress { turn_id: String, call_id: String, line: String },
    ToolFinished { turn_id: String, call_id: String, ok: bool, summary: String, error: Option<String> },
    ConfirmationRequired { turn_id: String, call_id: String, tool: String, title: String, risk: &'static str, details: String },
    TurnFinished { turn_id: String, status: &'static str, error: Option<String> },
    /// Waiting for the provider's rate limit.
    Waiting { turn_id: String, seconds: u64 },
    /// A message that did not come from a model turn (background job results).
    Report { text: String, tone: &'static str },
}
