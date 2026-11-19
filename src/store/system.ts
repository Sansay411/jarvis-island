// Live system state shown by the island: media session, volume, brightness,
// and real dominant album-art color extracted via a tiny offscreen canvas.

import { create } from "zustand";
import { errorMessage, ipc, isTauri, on } from "../lib/ipc";
import type { AudioState, DisplayBrightness, MediaState } from "../lib/types";
import { startHubFeed } from "./hub";
import { startTtsFeed } from "./tts";

interface SystemStore {
  media: MediaState | null;
  artwork: { trackKey: string; dataUrl: string | null } | null;
  /** Dominant vibrant `r, g, b` triplet sampled from real `artwork.dataUrl` (null when absent). */
  artworkRgb: string | null;
  audio: AudioState | null;
  brightness: DisplayBrightness[] | null;
  lastError: string | null;
  refreshControls: () => Promise<void>;
  setVolume: (percent: number) => Promise<void>;
  toggleMute: () => Promise<void>;
  setBrightness: (percent: number) => Promise<void>;
  seekMedia: (positionMs: number) => Promise<void>;
  clearError: () => void;
}

/** Extract a vibrant RGB color from real album art so the bottom rim can
 *  echo the active cover without hardcoded or synthetic gradients. */
function sampleArtworkRgb(dataUrl: string | null | undefined, onColor: (rgb: string | null) => void) {
  if (!dataUrl || typeof document === "undefined") {
    onColor(null);
    return;
  }
  const img = new Image();
  img.decoding = "async";
  img.onload = () => {
    try {
      const canvas = document.createElement("canvas");
      const size = 12;
      canvas.width = size;
      canvas.height = size;
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return onColor(null);
      ctx.drawImage(img, 0, 0, size, size);
      const { data } = ctx.getImageData(0, 0, size, size);
      let rSum = 0;
      let gSum = 0;
      let bSum = 0;
      let weightSum = 0;
      for (let i = 0; i < data.length; i += 4) {
        const r = data[i]!;
        const g = data[i + 1]!;
        const b = data[i + 2]!;
        const a = data[i + 3]!;
        if (a < 128) continue;
        const max = Math.max(r, g, b);
        const min = Math.min(r, g, b);
        const sat = max === 0 ? 0 : (max - min) / max;
        const lum = (r + g + b) / 3;
        // Prefer mid-bright saturated pixels over pitch black or pure white margins.
        const w = 0.25 + sat * 2.2 + (lum > 28 && lum < 235 ? 0.6 : 0);
        rSum += r * w;
        gSum += g * w;
        bSum += b * w;
        weightSum += w;
      }
      if (weightSum <= 0) return onColor(null);
      const rr = Math.round(rSum / weightSum);
      const gg = Math.round(gSum / weightSum);
      const bb = Math.round(bSum / weightSum);
      onColor(`${rr}, ${gg}, ${bb}`);
    } catch {
      onColor(null);
    }
  };
  img.onerror = () => onColor(null);
  img.src = dataUrl;
}

export const useSystem = create<SystemStore>((set, get) => ({
  media: null,
  artwork: null,
  artworkRgb: null,
  audio: null,
  brightness: null,
  lastError: null,

  refreshControls: async () => {
    if (!isTauri()) return;
    const [audio, brightness] = await Promise.allSettled([ipc.audio.get(), ipc.brightness.list()]);
    set({
      audio: audio.status === "fulfilled" ? audio.value : get().audio,
      brightness: brightness.status === "fulfilled" ? brightness.value : get().brightness ?? [],
    });
  },

  setVolume: async (percent) => {
    const v = Math.max(0, Math.min(100, Math.round(percent)));
    if (!isTauri()) {
      set((s) => ({
        audio: { volume: v, muted: v === 0, device: s.audio?.device ?? "Динамики" },
      }));
      return;
    }
    try {
      set({ audio: await ipc.audio.setVolume(v) });
    } catch (e) {
      set({ lastError: errorMessage(e) });
    }
  },

  toggleMute: async () => {
    const muted = !(get().audio?.muted ?? false);
    if (!isTauri()) {
      set((s) => ({
        audio: { volume: s.audio?.volume ?? 65, muted, device: s.audio?.device ?? "Динамики" },
      }));
      return;
    }
    try {
      set({ audio: await ipc.audio.setMute(muted) });
    } catch (e) {
      set({ lastError: errorMessage(e) });
    }
  },

  setBrightness: async (percent) => {
    const v = Math.max(0, Math.min(100, Math.round(percent)));
    if (!isTauri()) {
      set((s) => ({
        brightness: (s.brightness ?? [{ id: "internal", name: "Встроенный дисплей", kind: "internal", brightness: v, supported: true }]).map(
          (d, idx) => (idx === 0 ? { ...d, brightness: v } : d),
        ),
      }));
      return;
    }
    try {
      set({ brightness: await ipc.brightness.set(v) });
    } catch (e) {
      set({ lastError: errorMessage(e) });
    }
  },

  seekMedia: async (positionMs) => {
    const clamped = Math.max(0, Math.round(positionMs));
    set((s) =>
      s.media
        ? {
            media: {
              ...s.media,
              positionMs: s.media.durationMs ? Math.min(s.media.durationMs, clamped) : clamped,
              positionSampledAt: Date.now(),
            },
          }
        : s,
    );
    if (!isTauri()) return;
    try {
      await ipc.media.seek(clamped);
    } catch (e) {
      set({ lastError: errorMessage(e) });
    }
  },

  clearError: () => set({ lastError: null }),
}));

// Keep `artworkRgb` synced whenever `artwork` changes (both Tauri events & QA harness).
let prevArtworkUrl: string | null = null;
useSystem.subscribe((state) => {
  const nextUrl = state.artwork?.dataUrl ?? null;
  if (nextUrl === prevArtworkUrl) return;
  prevArtworkUrl = nextUrl;
  sampleArtworkRgb(nextUrl, (rgb) => useSystem.setState({ artworkRgb: rgb }));
});

/** Subscribe to native media, hub, and TTS feeds; call once per window. */
export async function startSystemFeeds() {
  void startHubFeed();
  void startTtsFeed();
  if (!isTauri()) return;

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

/** The last track that played, kept across restarts: the island's player
 *  shows it (dimmed, «Продолжить») when nothing is playing. */
export interface LastTrack {
  title: string;
  artist: string;
  sourceApp: string;
  art: string | null;
}

const LAST_KEY = "jarvis.lastTrack";

function readLast(): LastTrack | null {
  try {
    const v = localStorage.getItem(LAST_KEY);
    const t = v ? (JSON.parse(v) as LastTrack) : null;
    // The built-in radio of one test build (removed 2026-11-18) left itself
    // here as «Радио JARVIS»; it is not the user's player.
    if (t && isOwnSession(t.artist, t.sourceApp)) {
      localStorage.removeItem(LAST_KEY);
      return null;
    }
    return t;
  } catch {
    return null;
  }
}

function isOwnSession(artist: string, sourceApp: string): boolean {
  return artist.startsWith("Радио JARVIS") || /jarvis/i.test(sourceApp);
}

let lastTrack: LastTrack | null = readLast();
const lastListeners = new Set<() => void>();

export function getLastTrack(): LastTrack | null {
  return lastTrack;
}

export function onLastTrack(f: () => void): () => void {
  lastListeners.add(f);
  return () => lastListeners.delete(f);
}

useSystem.subscribe((state) => {
  const m = state.media;
  if (!m || !m.available || !(m.title || m.artist) || isOwnSession(m.artist, m.sourceApp)) return;
  const art = state.artwork && state.artwork.trackKey === m.trackKey ? state.artwork.dataUrl : null;
  const next: LastTrack = { title: m.title, artist: m.artist, sourceApp: m.sourceApp, art: art ?? (lastTrack?.title === m.title ? lastTrack.art : null) };
  if (lastTrack && lastTrack.title === next.title && lastTrack.artist === next.artist && lastTrack.art === next.art) return;
  lastTrack = next;
  try {
    localStorage.setItem(LAST_KEY, JSON.stringify(next));
  } catch {
    /* storage unavailable: remembered for this session only */
  }
  lastListeners.forEach((f) => f());
});

export function hasActiveMedia(m: MediaState | null): m is MediaState {
  return !!m && m.available && (m.status === "playing" || m.status === "paused") && !!(m.title || m.artist);
}
