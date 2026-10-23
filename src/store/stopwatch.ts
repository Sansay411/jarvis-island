// The stopwatch mirrored from the native side. Nothing is sent while it
// runs: the island counts from `startedAt` + `accumulatedMs` itself.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export interface StopwatchStatus {
  running: boolean;
  startedAt: string | null;
  accumulatedMs: number;
  laps: number[];
  active: boolean;
}

const IDLE: StopwatchStatus = { running: false, startedAt: null, accumulatedMs: 0, laps: [], active: false };

export const useStopwatch = create<StopwatchStatus>(() => IDLE);

export async function startStopwatchFeed() {
  await listen<StopwatchStatus>("stopwatch:changed", (e) => useStopwatch.setState(e.payload, true));
  try {
    useStopwatch.setState(await invoke<StopwatchStatus>("stopwatch_get"), true);
  } catch {
    /* changes arrive as events too */
  }
}

export const stopwatchAction = (action: "start" | "pause" | "lap" | "reset") => invoke("stopwatch_action", { action });

export function elapsedMs(s: StopwatchStatus): number {
  return s.accumulatedMs + (s.running && s.startedAt ? Math.max(0, Date.now() - Date.parse(s.startedAt)) : 0);
}

/** Re-render every `ms` while `on`. */
export function useTick(ms: number, on = true) {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!on) return;
    const id = window.setInterval(() => tick((n) => n + 1), ms);
    return () => window.clearInterval(id);
  }, [ms, on]);
}

/** «1:23» or «1:02:03»; with `tenths`, «1:23.4». */
export function clock(ms: number, tenths = false): string {
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  const base = h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
  return tenths ? `${base}.${Math.floor((ms % 1000) / 100)}` : base;
}
