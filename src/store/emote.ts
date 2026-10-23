// One-shot expressions of the eyes: a wink at «спасибо», hearts at
// «молодец», a laugh, surprise, a head shake. Set from the native side
// (`island:emote`) or the island itself; each clears itself.

import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";

export type Emote = "wink" | "heart" | "laugh" | "surprised" | "shake" | "boop";

const DURATION: Record<Emote, number> = {
  wink: 900,
  heart: 1800,
  laugh: 1400,
  surprised: 900,
  shake: 700,
  boop: 1100,
};

interface EmoteStore {
  emote: Emote | null;
  /** Bumps on every emote, so the same one twice in a row replays. */
  seq: number;
}

export const useEmote = create<EmoteStore>(() => ({ emote: null, seq: 0 }));

let clear: number | undefined;

export function emote(kind: Emote) {
  window.clearTimeout(clear);
  useEmote.setState((s) => ({ emote: kind, seq: s.seq + 1 }));
  clear = window.setTimeout(() => useEmote.setState({ emote: null }), DURATION[kind]);
}

export async function startEmoteFeed() {
  await listen<Emote>("island:emote", (e) => emote(e.payload));
}
