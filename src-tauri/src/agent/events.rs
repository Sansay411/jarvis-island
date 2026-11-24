//! Events streamed to the island while the agent works.
//! Mirrored in src/lib/agent-types.ts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EVT_AGENT: &str = "agent:event";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AgentEvent {
    /// `ambient`: a voice phrase heard without the name — the chat shows it
    /// only once the model answers it (it may finish as `ignored`).
    TurnStarted {
        turn_id: String,
        conversation_id: String,
        user_text: String,
        voice: bool,
        ambient: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        area: Option<crate::area::PendingAreaPreview>,
    },
    Phase { turn_id: String, phase: &'static str },
    TextDelta { turn_id: String, text: String },
    #[serde(rename = "plan_updated")]
    PlanUpdated { turn_id: String, steps: Vec<PlanStep> },
    ToolStarted { turn_id: String, call_id: String, tool: String, title: String, args: Value },
    ToolProgress { turn_id: String, call_id: String, line: String },
    ToolFinished { turn_id: String, call_id: String, ok: bool, summary: String, error: Option<String> },
    ConfirmationRequired { turn_id: String, call_id: String, tool: String, title: String, risk: &'static str, details: String },
    TurnFinished { turn_id: String, status: &'static str, error: Option<String> },
    /// Waiting for the provider's rate limit.
    Waiting { turn_id: String, seconds: u64 },
    /// A message that did not come from a model turn (background job results).
    Report { text: String, tone: &'static str },
    /// Quiet status notice shown in the chat without entering LLM history.
    Notice { text: String, tone: &'static str },
}
