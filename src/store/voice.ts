// Voice session state mirrored from the native voice manager. Audio levels
// are kept in motion values (updated ~30×/s) so the visualiser animates
// without re-rendering React.

import { create } from "zustand";
import { motionValue } from "motion/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { useIsland } from "./island";

export type VoiceState = "idle" | "connecting" | "listening" | "hearing" | "thinking" | "speaking" | "error";

interface VoiceStore {
  state: VoiceState;
  error: string | null;
  userLine: string;
  assistantLine: string;
}

export const useVoice = create<VoiceStore>(() => ({ state: "idle", error: null, userLine: "", assistantLine: "" }));

export const inputLevel = motionValue(0);
export const outputLevel = motionValue(0);

let closeTimer: number | undefined;

export async function startVoiceFeed() {
  await listen<{ state: VoiceState; error: string | null }>("voice:state", (e) => {
    const { state, error } = e.payload;
    const patch: Partial<VoiceStore> = { state, error: error ?? null };
    // A new utterance starts a new exchange. (Not on "thinking": the local
    // engine passes through it between spoken phrases of one answer.)
    if (state === "hearing") {
      patch.userLine = "";
      patch.assistantLine = "";
    }
    useVoice.setState(patch);
    window.clearTimeout(closeTimer);
    const island = useIsland.getState();
    if (state === "idle") {
      island.set({ voiceOpen: false });
    } else if (state === "error") {
      island.set({ voiceOpen: true });
      closeTimer = window.setTimeout(() => useIsland.getState().set({ voiceOpen: false }), 4000);
    } else {
      island.set({ voiceOpen: true });
    }
    if (state === "idle" || state === "error") {
      inputLevel.set(0);
      outputLevel.set(0);
    }
  });
  await listen<{ input: number; output: number }>("voice:level", (e) => {
    inputLevel.set(e.payload.input);
    outputLevel.set(e.payload.output);
  });
  await listen<{ role: "user" | "assistant"; text: string; final: boolean }>("voice:transcript", (e) => {
    const { role, text, final } = e.payload;
    useVoice.setState((s) =>
      role === "user"
        ? { userLine: final ? text : s.userLine + text }
        : { assistantLine: final ? text : s.assistantLine + text },
    );
  });
}

export async function toggleVoice() {
  try {
    await invoke<boolean>("voice_toggle");
  } catch (e) {
    useVoice.setState({ state: "error", error: errorMessage(e) });
    useIsland.getState().set({ voiceOpen: true });
    window.clearTimeout(closeTimer);
    closeTimer = window.setTimeout(() => useIsland.getState().set({ voiceOpen: false }), 4000);
  }
}
