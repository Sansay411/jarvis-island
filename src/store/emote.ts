// One-shot expressions of the eyes and what JARVIS is busy with (one kind
// of animation per kind of tool: searching, reading, typing, clicking,
// looking at the screen, music, calendar, memory, code…). Expressions
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

export type Activity =
  | "search"
  | "read"
  | "type"
  | "click"
  | "look"
  | "draw"
  | "files"
  | "music"
  | "calendar"
  | "memory"
  | "time"
  | "system"
  | "code"
  | "connect"
  | "speak"
  | "work";

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
  { id: "click", label: "Нажимает" },
  { id: "look", label: "Смотрит на экран" },
  { id: "draw", label: "Показывает" },
  { id: "files", label: "Файлы" },
  { id: "music", label: "Музыка" },
  { id: "calendar", label: "Календарь" },
  { id: "memory", label: "Память" },
  { id: "time", label: "Таймер" },
  { id: "system", label: "Система" },
  { id: "code", label: "Код и терминал" },
  { id: "connect", label: "Подключение" },
  { id: "speak", label: "Говорит" },
  { id: "work", label: "Работает" },
];

const TOOL_ACTIVITY: Record<string, Activity> = {
  // Search & web discovery
  web_search: "search",
  apps_find: "search",
  weather: "search",
  location: "search",

  // Reading pages, documents, UI tree, clipboard and context
  web_fetch: "read",
  file_read: "read",
  browser_read_page: "read",
  ui_inspect: "read",
  clipboard_read: "read",
  clipboard_history: "read",
  selection_grab: "read",
  context_snapshot: "read",
  windows_list: "read",
  browser_tabs: "read",

  // Typing & editing
  input_type: "type",
  input_key: "type",
  clipboard_write: "type",

  // Clicking, scrolling, switching & launching
  input_click: "click",
  input_scroll: "click",
  ui_act: "click",
  browser_switch_tab: "click",
  browser_close_tab: "click",
  window_action: "click",
  window_layout: "click",
  app_launch: "click",
  open_target: "click",

  // Visual inspection of the screen
  screen_capture: "look",

  // Drawing guides, rings and callouts on screen
  screen_point: "draw",
  guide_step: "draw",
  explain_card: "draw",

  // Files & folders
  files_search: "files",
  file_action: "files",

  // Music & media playback
  media_control: "music",

  // Calendar, planner, tasks & growth
  calendar_manage: "calendar",
  tasks_manage: "calendar",
  mission_plan: "calendar",
  growth: "calendar",

  // Long-term memory
  memory_save: "memory",
  memory_forget: "memory",

  // Reminders, timers, focus & waits
  reminder_create: "time",
  reminders_list: "time",
  reminder_cancel: "time",
  stopwatch: "time",
  focus_session: "time",
  wait: "time",

  // System controls & diagnostics
  system_volume: "system",
  app_volume: "system",
  display_brightness: "system",
  bluetooth_battery: "system",
  system_diagnostics: "system",

  // Code, shell, terminal & dev agents
  shell_run: "code",
  dev_tools_status: "code",
  projects_list: "code",
  vscode_open: "code",
  coding_agent_run: "code",
  job_status: "code",
  job_cancel: "code",
  terminal_open: "code",

  // Speech output
  speak_aloud: "speak",
};

/** What a running tool looks like on the face. Unknown tools fall back to `work`. */
export function activityOf(tool: string): Activity {
  const name = tool.trim();
  if (name.startsWith("mcp__") || name === "mcp") return "connect";
  return TOOL_ACTIVITY[name] ?? "work";
}

/** The least time one tool's animation plays, so quick calls in a row read
 *  as a steady rhythm instead of a flicker. */
export const ACTIVITY_HOLD_MS = 700;

/** What the face should show when the running tool becomes `next`: a first
 *  activity starts at once, a change waits until the shown one has played
 *  `hold` ms (`wait` is the rest; ask again then). */
export function activityStep(
  shown: Activity | null,
  since: number,
  next: Activity | null,
  now: number,
  hold = ACTIVITY_HOLD_MS,
): { show: Activity | null; wait: number } {
  if (next === shown || shown === null) return { show: next, wait: 0 };
  const left = hold - (now - since);
  return left > 0 ? { show: shown, wait: left } : { show: next, wait: 0 };
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
