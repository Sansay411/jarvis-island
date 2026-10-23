// One-shot expressions of the eyes and what JARVIS is busy with (looking
// up while it searches, reading, typing, scanning the screen). Expressions
// come from the native side (`island:emote`), from the agent's own events
// (a long job done → stars, cancelled → sad, a failed step → a squint), the
// stopwatch and the voice session; each clears itself.

import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type { AgentEvent } from "../lib/agent-types";

export type Emote =
  | "wink"
  | "heart"
  | "laugh"
  | "surprised"
  | "shake"
  | "boop"
  | "star"
  | "sad"
  | "determined"
  | "squint"
  | "confused"
  | "dizzy"
  | "yawn"
  | "wake";

export type Activity = "search" | "read" | "type" | "scan";

const DURATION: Record<Emote, number> = {
  wink: 900,
  heart: 1800,
  laugh: 1400,
  surprised: 900,
  shake: 700,
  boop: 1100,
  star: 1700,
  sad: 2200,
  determined: 1500,
  squint: 1400,
  confused: 1800,
  dizzy: 1500,
  yawn: 2000,
  wake: 800,
};

/** Names and order for the gallery in Settings. */
export const EMOTES: { id: Emote; label: string }[] = [
  { id: "wink", label: "Подмигнуть" },
  { id: "heart", label: "Сердечки" },
  { id: "laugh", label: "Смех" },
  { id: "star", label: "Звёздочки" },
  { id: "surprised", label: "Удивление" },
  { id: "determined", label: "Решимость" },
  { id: "squint", label: "Подозрение" },
  { id: "confused", label: "Растерянность" },
  { id: "sad", label: "Грусть" },
  { id: "dizzy", label: "Головокружение" },
  { id: "yawn", label: "Зевок" },
  { id: "wake", label: "Пробуждение" },
  { id: "shake", label: "Нет-нет" },
  { id: "boop", label: "Тык" },
];

export const ACTIVITIES: { id: Activity; label: string }[] = [
  { id: "search", label: "Ищет" },
  { id: "read", label: "Читает" },
  { id: "type", label: "Печатает" },
  { id: "scan", label: "Смотрит на экран" },
];

/** What a running tool looks like on the face. */
export function activityOf(tool: string): Activity | null {
  if (["web_search", "files_search", "apps_find", "projects_list", "reminders_list"].includes(tool)) return "search";
  if (["web_fetch", "file_read", "browser_read_page", "clipboard_read", "file_action"].includes(tool)) return "read";
  if (["input_type", "input_key", "clipboard_write", "shell_run"].includes(tool)) return "type";
  if (["screen_capture", "ui_inspect", "context_snapshot", "windows_list", "browser_tabs"].includes(tool)) return "scan";
  return null;
}

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

/** Tools started and failed in the running turn. */
const turns = new Map<string, { started: number; failed: number }>();

function onAgent(ev: AgentEvent) {
  switch (ev.type) {
    case "toolStarted": {
      const t = turns.get(ev.turnId) ?? { started: 0, failed: 0 };
      t.started += 1;
      turns.set(ev.turnId, t);
      // A job that keeps going: the head spins a little.
      if (t.started === 6) emote("dizzy");
      break;
    }
    case "toolFinished":
      if (ev.summary === "Отклонено пользователем") emote("sad");
      else if (!ev.ok) {
        const t = turns.get(ev.turnId);
        if (t) t.failed += 1;
        emote("squint");
      }
      break;
    case "turnFinished": {
      const t = turns.get(ev.turnId);
      turns.delete(ev.turnId);
      if (ev.status === "cancelled") emote("sad");
      else if (ev.status === "failed") emote("confused");
      else if (ev.status === "completed" && t && t.started >= 3) emote("star");
      break;
    }
  }
}

export async function startEmoteFeed() {
  await listen<Emote>("island:emote", (e) => emote(e.payload));
  await listen<AgentEvent>("agent:event", (e) => onAgent(e.payload));
  // Started from zero (not a lap mark): a determined nod.
  await listen<{ running: boolean; accumulatedMs: number; startedAt: string | null; laps: number[] }>("stopwatch:changed", (e) => {
    const s = e.payload;
    const fresh = s.startedAt && Date.now() - Date.parse(s.startedAt) < 1500;
    if (s.running && s.accumulatedMs === 0 && s.laps.length === 0 && fresh) emote("determined");
  });
  let timers = -1;
  await listen<{ kind: string; firedAt: string | null }[]>("reminders:changed", (e) => {
    const n = e.payload.filter((r) => r.kind === "timer" && !r.firedAt).length;
    if (timers >= 0 && n > timers) emote("determined");
    timers = n;
  });
  // The voice session: eyes open when the name is heard, a yawn when it dozes off.
  let voice = "";
  await listen<{ state: string }>("voice:state", (e) => {
    const next = e.payload.state;
    if (voice === "dormant" && (next === "listening" || next === "hearing")) emote("wake");
    else if ((voice === "listening" || voice === "speaking") && next === "dormant") emote("yawn");
    voice = next;
  });
}
