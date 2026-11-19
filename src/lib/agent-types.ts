// Agent protocol mirrored from src-tauri/src/agent/events.rs.

export type AgentPhase = "thinking" | "acting" | "verifying";
export type RiskLevel = "low" | "moderate" | "high";
export type PlanStepStatus = "pending" | "running" | "done" | "failed";

export interface PlanStep {
  id: string;
  title: string;
  status: PlanStepStatus;
}

export type AgentEvent =
  | { type: "turnStarted"; turnId: string; conversationId: string; userText: string; voice?: boolean }
  | { type: "phase"; turnId: string; phase: AgentPhase }
  | { type: "textDelta"; turnId: string; text: string }
  | { type: "toolStarted"; turnId: string; callId: string; tool: string; title: string; args: unknown }
  | { type: "toolProgress"; turnId: string; callId: string; line: string }
  | {
      type: "toolFinished";
      turnId: string;
      callId: string;
      ok: boolean;
      summary: string;
      error: string | null;
    }
  | {
      type: "confirmationRequired";
      turnId: string;
      callId: string;
      tool: string;
      title: string;
      risk: RiskLevel;
      details: string;
    }
  | { type: "plan_updated"; turnId: string; steps: PlanStep[] }
  | { type: "planUpdated"; turnId: string; steps: PlanStep[] }
  | { type: "turnFinished"; turnId: string; status: "completed" | "cancelled" | "failed"; error: string | null }
  | { type: "turn_done"; turnId: string; status?: "completed" | "cancelled" | "failed"; error?: string | null }
  | { type: "report"; text: string; tone: "ok" | "error" }
  | { type: "waiting"; turnId: string; seconds: number };

export interface AgentStatus {
  configured: boolean;
  provider: string;
  model: string;
  problem: string | null;
}

export interface SendResult {
  turnId: string;
  conversationId: string;
}

export interface HistoryMessage {
  role: "user" | "assistant" | "tool";
  text: string;
  toolTitle: string | null;
  toolOk: boolean | null;
  createdAt: string;
}
