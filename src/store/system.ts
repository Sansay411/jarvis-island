// Live system state shown by the island: media session, volume, brightness.
// Media updates are pushed by the native GSMTC watcher; volume/brightness
// are fetched when the island opens and after every change (read-back).

import { create } from "zustand";
import { errorMessage, ipc, on } from "../lib/ipc";
import type { AudioState, DisplayBrightness, MediaState } from "../lib/types";

interface SystemStore {
  media: MediaState | null;
  artwork: { trackKey: string; dataUrl: string | null } | null;
  audio: AudioState | null;
  brightness: DisplayBrightness[] | null;
  lastError: string | null;
  refreshControls: () => Promise<void>;
  setVolume: (percent: number) => Promise<void>;
  toggleMute: () => Promise<void>;
  setBrightness: (percent: number) => Promise<void>;
  clearError: () => void;
}

export const useSystem = create<SystemStore>((set, get) => ({
  media: null,
  artwork: null,
  audio: null,
  brightness: null,
  lastError: null,

  refreshControls: async () => {
    const [audio, brightness] = await Promise.allSettled([ipc.audio.get(), ipc.brightness.list()]);
    set({
      audio: audio.status === "fulfilled" ? audio.value : get().audio,
      brightness: brightness.status === "fulfilled" ? brightness.value : [],
    });
  },

  setVolume: async (percent) => {
    try {
      set({ audio: await ipc.audio.setVolume(percent) });
    } catch (e) {
      set({ lastError: errorMessage(e) });
    }
  },

  toggleMute: async () => {
    const muted = !(get().audio?.muted ?? false);
    try {
      set({ audio: await ipc.audio.setMute(muted) });
    } catch (e) {
      set({ lastError: errorMessage(e) });
    }
  },

  setBrightness: async (percent) => {
    try {
      set({ brightness: await ipc.brightness.set(percent) });
    } catch (e) {
      set({ lastError: errorMessage(e) });
    }
  },

  clearError: () => set({ lastError: null }),
}));

/** Subscribe to native media events; call once per window. */
export async function startSystemFeeds() {
  const s = useSystem.setState;
  await on("media:changed", (media) => s({ media }));
  await on("media:artwork", (artwork) => s({ artwork }));
  // Initial state (events only fire on change).
  try {
    const media = await ipc.media.get();
    s({ media });
    if (media.available) s({ artwork: await ipc.media.artwork() });
  } catch {
    /* GSMTC unavailable: the media card simply stays hidden */
  }
}

export function hasActiveMedia(m: MediaState | null): m is MediaState {
  return !!m && m.available && (m.status === "playing" || m.status === "paused") && !!(m.title || m.artist);
}
