// Secure key prompt requested by `connect_service` in the island.
// Values entered here go straight to Rust (`mcp_secret_submit`) and Credential
// Manager — never through the chat history or the model.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage, isTauri } from "../lib/ipc";

export interface SecretFieldPrompt {
  key: string;
  label: string;
  hint?: string | null;
  link?: string | null;
  placeholder?: string | null;
  optional?: boolean;
}

export interface SecretPromptPayload {
  promptId: string;
  serviceId: string;
  serviceName: string;
  description: string;
  note?: string | null;
  fields: SecretFieldPrompt[];
}

interface McpPromptStore {
  prompt: SecretPromptPayload | null;
  height: number;
  submitting: boolean;
  error: string | null;
  setHeight: (h: number) => void;
  submit: (values: Record<string, string>) => Promise<void>;
  cancel: () => Promise<void>;
}

export const useMcpPrompt = create<McpPromptStore>((set, get) => ({
  prompt: null,
  height: 248,
  submitting: false,
  error: null,
  setHeight: (h) => set((s) => (Math.abs(s.height - h) >= 2 ? { height: h } : s)),
  submit: async (values) => {
    const p = get().prompt;
    if (!p || get().submitting) return;
    set({ submitting: true, error: null });
    if (!isTauri()) {
      set({ prompt: null, submitting: false, error: null });
      return;
    }
    try {
      await invoke("mcp_secret_submit", { promptId: p.promptId, values });
      set({ prompt: null, submitting: false, error: null });
    } catch (e) {
      set({ submitting: false, error: errorMessage(e) });
    }
  },
  cancel: async () => {
    const p = get().prompt;
    if (!p) return;
    set({ prompt: null, submitting: false, error: null });
    if (!isTauri()) return;
    try {
      await invoke("mcp_secret_submit", { promptId: p.promptId, values: null });
    } catch {
      /* prompt already closed on backend */
    }
  },
}));

export async function startMcpPromptFeed() {
  if (!isTauri()) return;
  await listen<SecretPromptPayload>("mcp:secret-prompt", (e) => {
    useMcpPrompt.setState({ prompt: e.payload, submitting: false, error: null });
  });
  await listen<{ promptId?: string }>("mcp:secret-prompt-closed", (e) => {
    const cur = useMcpPrompt.getState().prompt;
    if (cur && (!e.payload?.promptId || cur.promptId === e.payload.promptId)) {
      useMcpPrompt.setState({ prompt: null, submitting: false, error: null });
    }
  });
}
