// Live neural TTS playback state mirrored from `ipc_tts.rs` (`tts:state`).
// When `speaking` is true, the Island animates Jarvis's eyes in `"speaking"`
// mood and surfaces a 1-click Stop Speech control.

import { create } from "zustand";
import { ipc, isTauri, on } from "../lib/ipc";
import type { TtsBenchResult, TtsSpeakOptions, TtsStatePayload } from "../lib/types";
import { outputLevel } from "./voice";

interface TtsStore extends TtsStatePayload {
  speak: (text: string, opts?: TtsSpeakOptions) => Promise<void>;
  stop: () => Promise<void>;
  benchmark: (voice?: string | null) => Promise<TtsBenchResult | null>;
}

let simTimer: number | undefined;
let simRaf = 0;

function stopBrowserSimulation() {
  window.clearTimeout(simTimer);
  if (simRaf) cancelAnimationFrame(simRaf);
  simRaf = 0;
  outputLevel.set(0);
}

function startBrowserSimulation(text: string, voice: string) {
  stopBrowserSimulation();
  const startedAt = performance.now();
  const step = (now: number) => {
    const t = (now - startedAt) / 1000;
    // Syllabic speech envelope for browser QA preview when native audio isn't attached.
    const env = Math.max(0.08, Math.min(0.95, 0.42 + 0.32 * Math.sin(t * 11.5) + 0.2 * Math.sin(t * 4.7)));
    outputLevel.set(env);
    simRaf = requestAnimationFrame(step);
  };
  simRaf = requestAnimationFrame(step);
  const durationMs = Math.min(12_000, Math.max(2200, text.length * 55));
  useTts.setState({ speaking: true, text, voice, latencyMs: 168 });
  simTimer = window.setTimeout(() => {
    stopBrowserSimulation();
    useTts.setState({ speaking: false, text: "" });
  }, durationMs);
}

export const useTts = create<TtsStore>((set) => ({
  speaking: false,
  text: "",
  voice: "edge:ru-RU-DmitryNeural",
  latencyMs: null,

  speak: async (text, opts) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    if (!isTauri()) {
      startBrowserSimulation(trimmed, opts?.voice ?? "edge:ru-RU-DmitryNeural");
      return;
    }
    try {
      const out = await ipc.tts.speak(trimmed, opts);
      if (out.started) {
        set({ speaking: true, text: trimmed, voice: out.voice });
      }
    } catch {
      /* TTS errors surface via status/settings without crashing the Island */
    }
  },

  stop: async () => {
    stopBrowserSimulation();
    set({ speaking: false, text: "" });
    if (!isTauri()) return;
    try {
      await ipc.tts.stop();
    } catch {
      /* ignore */
    }
  },

  benchmark: async (voice) => {
    if (!isTauri()) {
      return {
        voice: voice ?? "edge:ru-RU-DmitryNeural",
        firstChunkMs: 174,
        totalMs: 612,
        samples: 24000,
        cachedMs: 6,
      };
    }
    try {
      return await ipc.tts.benchmark(voice);
    } catch {
      return null;
    }
  },
}));

let feedStarted = false;

export async function startTtsFeed() {
  if (feedStarted) return;
  feedStarted = true;
  if (!isTauri()) return;

  await on("tts:state", (payload) => {
    useTts.setState({
      speaking: payload.speaking,
      text: payload.text ?? "",
      voice: payload.voice || "edge:ru-RU-DmitryNeural",
      latencyMs: payload.latencyMs ?? null,
    });
    if (!payload.speaking) {
      outputLevel.set(0);
    }
  });

  try {
    const cur = await ipc.tts.status();
    useTts.setState({
      speaking: cur.speaking,
      text: cur.text ?? "",
      voice: cur.voice || "edge:ru-RU-DmitryNeural",
      latencyMs: cur.latencyMs ?? null,
    });
  } catch {
    /* backend may still be initializing */
  }
}
