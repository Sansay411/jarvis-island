// Conversation state for the workspace, fed by native agent events.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import type { AgentEvent, AgentPhase, AgentStatus, HistoryMessage, RiskLevel, SendResult } from "../lib/agent-types";
import type { EyeMood } from "../island/Eyes";

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

interface AgentStore {
  items: ChatItem[];
  activeTurn: string | null;
  phase: AgentPhase | null;
  status: AgentStatus | null;
  flash: "happy" | "error" | null;
  loadStatus: () => Promise<void>;
  loadHistory: () => Promise<void>;
  send: (text: string, attachments: string[]) => Promise<void>;
  cancel: () => Promise<void>;
  confirm: (callId: string, approved: boolean) => Promise<void>;
  newConversation: () => Promise<void>;
}

let seq = 0;
const uid = () => `l${Date.now().toString(36)}${(seq++).toString(36)}`;
let flashTimer: number | undefined;

export const useAgent = create<AgentStore>((set, get) => ({
  items: [],
  activeTurn: null,
  phase: null,
  status: null,
  flash: null,

  loadStatus: async () => {
    try {
      set({ status: await invoke<AgentStatus>("agent_status") });
    } catch (e) {
      set({ status: { configured: false, provider: "", model: "", problem: errorMessage(e) } });
    }
  },

  loadHistory: async () => {
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
    try {
      const res = await invoke<SendResult>("agent_send", { text, attachments });
      set({ activeTurn: res.turnId, phase: "thinking" });
    } catch (e) {
      set((s) => ({ items: [...s.items, { kind: "notice", id: uid(), tone: "error", text: errorMessage(e) }] }));
    }
  },

  cancel: async () => {
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
    await invoke("agent_confirm", { callId, approved });
  },

  newConversation: async () => {
    if (get().activeTurn) return;
    await invoke("agent_new_conversation");
    set({ items: [] });
  },
}));

function apply(ev: AgentEvent) {
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
          items: known ? s.items : [...s.items, { kind: "user", id: uid(), text: ev.userText, attachments: [] }],
        };
      });
      break;
    case "phase":
      set({ phase: ev.phase });
      break;
    case "textDelta":
      set((s) => {
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
    case "turnFinished": {
      window.clearTimeout(flashTimer);
      const flash = ev.status === "completed" ? "happy" : ev.status === "failed" ? "error" : null;
      set((s) => ({
        activeTurn: null,
        phase: null,
        flash,
        items: [
          ...s.items.map((it) => (it.kind === "assistant" && it.turnId === ev.turnId ? { ...it, streaming: false } : it)),
          ...(ev.status === "failed" && ev.error
            ? [{ kind: "notice" as const, id: uid(), tone: "error" as const, text: ev.error }]
            : ev.status === "cancelled"
              ? [{ kind: "notice" as const, id: uid(), tone: "info" as const, text: "Остановлено" }]
              : []),
        ],
      }));
      if (flash) flashTimer = window.setTimeout(() => set({ flash: null }), flash === "happy" ? 1600 : 2400);
      break;
    }
  }
}

export async function startAgentFeed() {
  await listen<AgentEvent>("agent:event", (e) => apply(e.payload));
  await listen("settings:changed", () => void useAgent.getState().loadStatus());
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
