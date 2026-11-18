// Conversation state for the workspace, fed by native agent events.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import type {
  AgentEvent,
  AgentPhase,
  AgentStatus,
  HistoryMessage,
  PlanStep,
  PlanStepStatus,
  RiskLevel,
  SendResult,
} from "../lib/agent-types";
import { ensureWorkspaceFeeds, hasTauri, stripMarkdownForSpeech, workspaceIpc } from "../lib/workspace-ipc";
import type { EyeMood } from "../island/Eyes";
import { usePrefs } from "./prefs";
import { useVoice } from "./voice";

export type { PlanStep, PlanStepStatus };
export type ToolStatus = "running" | "ok" | "failed" | "awaiting" | "declined";

export type ChatItem =
  | { kind: "user"; id: string; text: string; attachments: string[] }
  | { kind: "assistant"; id: string; turnId: string; text: string; streaming: boolean }
  | {
      kind: "tool";
      id: string;
      turnId: string;
      tool: string;
      title: string;
      status: ToolStatus;
      args: unknown;
      summary?: string;
      error?: string | null;
      risk?: RiskLevel;
      details?: string;
      /** Last lines of live output while running. */
      progress?: string[];
    }
  | { kind: "notice"; id: string; tone: "error" | "info"; text: string };

export interface AgentStore {
  items: ChatItem[];
  activeTurn: string | null;
  phase: AgentPhase | null;
  status: AgentStatus | null;
  flash: "happy" | "error" | null;
  /** Epoch ms until which the provider asked us to wait (rate limit). */
  waitUntil: number | null;
  /** Live multi-step mission plan published by the agent during a turn. */
  planSteps: PlanStep[];
  planTurnId: string | null;
  loadStatus: () => Promise<void>;
  loadHistory: () => Promise<void>;
  send: (text: string, attachments: string[]) => Promise<void>;
  cancel: () => Promise<void>;
  confirm: (callId: string, approved: boolean) => Promise<void>;
  newConversation: () => Promise<void>;
  clearPlan: () => void;
}

let seq = 0;
const uid = () => `l${Date.now().toString(36)}${(seq++).toString(36)}`;
let flashTimer: number | undefined;
/** Turns already reported finished: a fast turn can finish before the
 * `agent_send` reply arrives, and must not be marked active again by it. */
const finishedTurns: string[] = [];

function normalizeStepStatus(raw: string): PlanStepStatus {
  if (raw === "running" || raw === "in_progress" || raw === "active") return "running";
  if (raw === "done" || raw === "completed" || raw === "ok") return "done";
  if (raw === "failed" || raw === "error") return "failed";
  return "pending";
}

export const useAgent = create<AgentStore>((set, get) => ({
  items: [],
  activeTurn: null,
  phase: null,
  status: null,
  flash: null,
  waitUntil: null,
  planSteps: [],
  planTurnId: null,

  loadStatus: async () => {
    if (!hasTauri()) {
      set((s) => ({
        status: s.status ?? { configured: true, provider: "DashScope", model: "qwen3.8-max", problem: null },
      }));
      return;
    }
    try {
      set({ status: await invoke<AgentStatus>("agent_status") });
    } catch (e) {
      set({ status: { configured: false, provider: "", model: "", problem: errorMessage(e) } });
    }
  },

  loadHistory: async () => {
    if (!hasTauri()) return;
    try {
      const history = await invoke<HistoryMessage[]>("agent_history");
      const items: ChatItem[] = history.map((m) =>
        m.role === "user"
          ? { kind: "user", id: uid(), text: m.text, attachments: [] }
          : m.role === "assistant"
            ? { kind: "assistant", id: uid(), turnId: "", text: m.text, streaming: false }
            : {
                kind: "tool",
                id: uid(),
                turnId: "",
                tool: "",
                title: m.toolTitle ?? "",
                status: m.toolOk ? "ok" : "failed",
                args: null,
                summary: m.text,
              },
      );
      set({ items });
    } catch {
      /* empty history is fine */
    }
  },

  send: async (text, attachments) => {
    if (get().activeTurn) return;
    set((s) => ({ items: [...s.items, { kind: "user", id: uid(), text, attachments }] }));
    if (!hasTauri()) {
      const turnId = uid();
      set({ activeTurn: turnId, phase: "thinking" });
      window.setTimeout(() => {
        set((s) => ({
          activeTurn: null,
          phase: null,
          items: [
            ...s.items,
            {
              kind: "assistant",
              id: uid(),
              turnId,
              text: `Выполняю запрос: **${text}**. Все исполнительные модули активны.`,
              streaming: false,
            },
          ],
        }));
        if (usePrefs.getState().autoSpeakReplies) {
          void workspaceIpc.tts.speak(`Выполняю запрос: ${text}. Все исполнительные модули активны.`, { interrupt: true });
        }
      }, 450);
      return;
    }
    try {
      const res = await invoke<SendResult>("agent_send", { text, attachments });
      if (res && !finishedTurns.includes(res.turnId)) set({ activeTurn: res.turnId, phase: "thinking" });
    } catch (e) {
      set((s) => ({ items: [...s.items, { kind: "notice", id: uid(), tone: "error", text: errorMessage(e) }] }));
    }
  },

  cancel: async () => {
    if (!hasTauri()) {
      set({ activeTurn: null, phase: null });
      return;
    }
    try {
      await invoke("agent_cancel");
    } catch (e) {
      set((s) => ({ items: [...s.items, { kind: "notice", id: uid(), tone: "error", text: errorMessage(e) }] }));
    }
  },

  confirm: async (callId, approved) => {
    set((s) => ({
      items: s.items.map((it) =>
        it.kind === "tool" && it.id === callId ? { ...it, status: approved ? "running" : "declined" } : it,
      ),
    }));
    if (!hasTauri()) {
      window.setTimeout(() => {
        set((s) => ({
          items: s.items.map((it) =>
            it.kind === "tool" && it.id === callId
              ? { ...it, status: approved ? "ok" : "declined", summary: approved ? "Подтверждено и выполнено" : "Отклонено пользователем" }
              : it,
          ),
        }));
      }, 300);
      return;
    }
    await invoke("agent_confirm", { callId, approved });
  },

  newConversation: async () => {
    if (get().activeTurn) return;
    if (hasTauri()) {
      await invoke("agent_new_conversation");
    }
    set({ items: [], planSteps: [], planTurnId: null });
  },

  clearPlan: () => set({ planSteps: [], planTurnId: null }),
}));

export function applyAgentEvent(ev: AgentEvent) {
  const set = useAgent.setState;
  switch (ev.type) {
    case "turnStarted":
      set((s) => {
        // Requests started outside the composer (CLI, voice) still appear in the chat.
        const last = [...s.items].reverse().find((it) => it.kind === "user");
        const known = last && last.kind === "user" && last.text === ev.userText;
        return {
          activeTurn: ev.turnId,
          phase: "thinking",
          planSteps: s.planTurnId === ev.turnId ? s.planSteps : [],
          planTurnId: ev.turnId,
          items: known ? s.items : [...s.items, { kind: "user", id: uid(), text: ev.userText, attachments: [] }],
        };
      });
      break;
    case "phase":
      set({ phase: ev.phase, waitUntil: null });
      break;
    case "waiting":
      set({ waitUntil: Date.now() + ev.seconds * 1000 });
      break;
    case "plan_updated":
    case "planUpdated":
      set({
        planTurnId: ev.turnId,
        planSteps: (ev.steps ?? []).map((st, idx) => ({
          id: st.id || `step-${idx + 1}`,
          title: st.title,
          status: normalizeStepStatus(st.status),
        })),
      });
      break;
    case "textDelta":
      set((s) => {
        if (s.waitUntil) s = { ...s, waitUntil: null };
        const last = s.items[s.items.length - 1];
        if (last && last.kind === "assistant" && last.turnId === ev.turnId && last.streaming) {
          const items = s.items.slice(0, -1);
          items.push({ ...last, text: last.text + ev.text });
          return { items };
        }
        return { items: [...s.items, { kind: "assistant", id: uid(), turnId: ev.turnId, text: ev.text, streaming: true }] };
      });
      break;
    case "toolStarted":
      set((s) => {
        // Close the current text segment so later text starts below the tool row.
        const items = s.items.map((it) => (it.kind === "assistant" && it.turnId === ev.turnId ? { ...it, streaming: false } : it));
        const existing = items.find((it) => it.kind === "tool" && it.id === ev.callId);
        if (existing) {
          return { items: items.map((it) => (it.kind === "tool" && it.id === ev.callId ? { ...it, status: "running" as const } : it)) };
        }
        return {
          phase: "acting",
          items: [...items, { kind: "tool", id: ev.callId, turnId: ev.turnId, tool: ev.tool, title: ev.title, status: "running", args: ev.args }],
        };
      });
      break;
    case "toolProgress":
      set((s) => ({
        items: s.items.map((it) =>
          it.kind === "tool" && it.id === ev.callId ? { ...it, progress: [...(it.progress ?? []), ev.line].slice(-4) } : it,
        ),
      }));
      break;
    case "confirmationRequired":
      set((s) => {
        const items = s.items.map((it) => (it.kind === "assistant" && it.turnId === ev.turnId ? { ...it, streaming: false } : it));
        return {
          items: [
            ...items.filter((it) => !(it.kind === "tool" && it.id === ev.callId)),
            {
              kind: "tool",
              id: ev.callId,
              turnId: ev.turnId,
              tool: ev.tool,
              title: ev.title,
              status: "awaiting",
              args: null,
              risk: ev.risk,
              details: ev.details,
            },
          ],
        };
      });
      break;
    case "toolFinished":
      set((s) => ({
        items: s.items.map((it) =>
          it.kind === "tool" && it.id === ev.callId
            ? { ...it, status: it.status === "declined" ? "declined" : ev.ok ? "ok" : "failed", summary: ev.summary, error: ev.error }
            : it,
        ),
      }));
      break;
    case "report": {
      window.clearTimeout(flashTimer);
      const flash = ev.tone === "ok" ? "happy" : "error";
      set((s) => ({ flash, items: [...s.items, { kind: "assistant", id: uid(), turnId: "", text: ev.text, streaming: false }] }));
      flashTimer = window.setTimeout(() => set({ flash: null }), 2200);
      break;
    }
    case "turnFinished":
    case "turn_done": {
      const status = ev.status ?? "completed";
      const errText = ev.error ?? null;
      finishedTurns.push(ev.turnId);
      if (finishedTurns.length > 20) finishedTurns.shift();
      window.clearTimeout(flashTimer);
      const flash = status === "completed" ? "happy" : status === "failed" ? "error" : null;

      const prevItems = useAgent.getState().items;
      const assistantReply = prevItems
        .filter((it): it is Extract<ChatItem, { kind: "assistant" }> => it.kind === "assistant" && it.turnId === ev.turnId)
        .map((it) => it.text.trim())
        .filter(Boolean)
        .join("\n\n");

      set((s) => ({
        activeTurn: null,
        phase: null,
        waitUntil: null,
        flash,
        planSteps:
          s.planTurnId === ev.turnId && status === "completed"
            ? s.planSteps.map((st) => (st.status === "running" ? { ...st, status: "done" } : st))
            : s.planSteps,
        items: [
          ...s.items.map((it) => (it.kind === "assistant" && it.turnId === ev.turnId ? { ...it, streaming: false } : it)),
          ...(status === "failed" && errText
            ? [{ kind: "notice" as const, id: uid(), tone: "error" as const, text: errText }]
            : status === "cancelled"
              ? [{ kind: "notice" as const, id: uid(), tone: "info" as const, text: "Остановлено" }]
              : []),
        ],
      }));

      if (flash) flashTimer = window.setTimeout(() => set({ flash: null }), flash === "happy" ? 1600 : 2400);

      // Auto-speak reply if enabled in preferences and not an active voice-mode turn
      if (status === "completed" && assistantReply && usePrefs.getState().autoSpeakReplies) {
        const vState = useVoice.getState().state;
        const isVoiceTurn = vState !== "idle" && vState !== "dormant";
        if (!isVoiceTurn) {
          const spoken = stripMarkdownForSpeech(assistantReply);
          if (spoken) {
            void invoke("tts_speak", { text: spoken, interrupt: true }).catch(() => {
              void workspaceIpc.tts.speak(spoken, { interrupt: true });
            });
          }
        }
      }
      break;
    }
  }
}

export async function startAgentFeed() {
  void ensureWorkspaceFeeds();
  if (hasTauri()) {
    await listen<AgentEvent>("agent:event", (e) => applyAgentEvent(e.payload));
    await listen("settings:changed", () => void useAgent.getState().loadStatus());
  }
  await useAgent.getState().loadStatus();
  await useAgent.getState().loadHistory();
}

/** Eye mood implied by the agent's state, or null when idle. */
export function useAgentMood(): EyeMood | null {
  const active = useAgent((s) => s.activeTurn);
  const flash = useAgent((s) => s.flash);
  const awaiting = useAgent((s) => s.items.some((it) => it.kind === "tool" && it.status === "awaiting"));
  if (active && awaiting) return "attentive";
  if (active) return "thinking";
  if (flash) return flash;
  return null;
}
