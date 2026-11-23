// The calendar on the island: the planner's events from a week back to four
// weeks ahead (reloaded whenever the planner or the Google sync changes
// them), what is on now and next, and the reminder card a few minutes before
// a block starts. Editing stays in the planner window; the island reads,
// reminds and starts focus.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../lib/ipc";
import type { TaskItem } from "../lib/types";
import { addDays, isoDate, parseLocal } from "../planner/dates";
import type { CalEvent } from "./planner";
import { localTime, type Reminder } from "./reminders";
import { usePrefs } from "./prefs";

export type { CalEvent };

/** Blocks a focus session fits: work, study and projects. */
export const focusable = (e: Pick<CalEvent, "category">) => e.category === "work" || e.category === "study" || e.category === "project";

export const CATEGORY_LABEL: Record<string, string> = {
  work: "Работа",
  study: "Учёба",
  sport: "Спорт",
  project: "Проекты",
  personal: "Личное",
  google: "Google Календарь",
};

export interface Upcoming {
  event: CalEvent;
  /** Whole minutes to the start; 0 once it has begun. */
  minutes: number;
  started: boolean;
  /** A focus session for it: the block's length, or what is left of it. */
  focusMin: number;
}

const clampFocus = (m: number) => Math.min(240, Math.max(5, Math.round(m / 5) * 5));

/** The day's events: all-day ones first, then by start. */
export function eventsOn(events: CalEvent[], iso: string): CalEvent[] {
  return events
    .filter((e) => (e.allDay ? e.start <= iso && iso < e.end : e.start.slice(0, 10) === iso))
    .sort((a, b) => Number(b.allDay) - Number(a.allDay) || a.start.localeCompare(b.start));
}

/** The timed block on now and the next one today (done ones skipped). */
export function nowAndNext(events: CalEvent[], now: Date): { current: CalEvent | null; next: CalEvent | null } {
  const t = now.getTime();
  let current: CalEvent | null = null;
  let next: CalEvent | null = null;
  for (const e of eventsOn(events, isoDate(now))) {
    if (e.allDay || e.done) continue;
    const s = parseLocal(e.start).getTime();
    if (s <= t && t < parseLocal(e.end).getTime()) current ??= e;
    else if (s > t) next ??= e;
  }
  return { current, next };
}

/** What the island reminds about: the earliest timed block starting within
 *  `lead` minutes, or one that began less than two minutes ago. */
export function upcoming(events: CalEvent[], now: Date, lead: number): Upcoming | null {
  if (lead <= 0) return null;
  const t = now.getTime();
  let best: Upcoming | null = null;
  let bestStart = Infinity;
  for (const e of events) {
    if (e.allDay || e.done) continue;
    const s = parseLocal(e.start).getTime();
    const end = parseLocal(e.end).getTime();
    if (end <= t || s - t > lead * 60_000 || t - s > 2 * 60_000 || s >= bestStart) continue;
    bestStart = s;
    const started = s <= t;
    best = { event: e, minutes: started ? 0 : Math.ceil((s - t) / 60_000), started, focusMin: clampFocus((end - Math.max(s, t)) / 60_000) };
  }
  return best;
}

/** A block for the focus deck: the one on now (focus to its end) or one
 *  starting within a quarter of an hour (focus for its whole length). */
export function focusBlock(events: CalEvent[], now: Date): { event: CalEvent; focusMin: number; started: boolean } | null {
  const { current, next } = nowAndNext(events, now);
  const t = now.getTime();
  if (current && focusable(current)) return { event: current, focusMin: clampFocus((parseLocal(current.end).getTime() - t) / 60_000), started: true };
  if (next && focusable(next) && parseLocal(next.start).getTime() - t <= 15 * 60_000) {
    return { event: next, focusMin: clampFocus((parseLocal(next.end).getTime() - parseLocal(next.start).getTime()) / 60_000), started: false };
  }
  return null;
}

export type EventState = "allday" | "past" | "now" | "next" | "later";

export type AgendaRow =
  | { kind: "event"; key: string; event: CalEvent; time: string | null; state: EventState }
  | { kind: "task"; key: string; task: TaskItem; time: string | null }
  | { kind: "reminder"; key: string; reminder: Reminder; time: string };

/** One day on the island: all-day events, then events, timed tasks and
 *  reminders by time, then tasks without a time, done tasks last. */
export function dayRows(events: CalEvent[], tasks: TaskItem[], reminders: Reminder[], iso: string, now: Date): AgendaRow[] {
  const today = isoDate(now);
  const t = now.getTime();
  const { current, next } = iso === today ? nowAndNext(events, now) : { current: null, next: null };
  const rows: Array<{ sort: string; row: AgendaRow }> = [];
  for (const e of eventsOn(events, iso)) {
    const time = e.allDay ? null : e.start.slice(11, 16);
    const state: EventState = e.allDay ? "allday" : e === current ? "now" : e === next ? "next" : parseLocal(e.end).getTime() <= t ? "past" : "later";
    rows.push({ sort: e.allDay ? "0" : `1${time}`, row: { kind: "event", key: `e-${e.id}`, event: e, time, state } });
  }
  for (const task of tasks) {
    const on = task.scheduledDate ? task.scheduledDate === iso : iso === today && !task.completed;
    if (on) rows.push({ sort: task.completed ? "9" : task.scheduledTime ? `1${task.scheduledTime}` : "5", row: { kind: "task", key: `t-${task.id}`, task, time: task.scheduledTime } });
  }
  for (const r of reminders) {
    if (r.done || r.kind !== "reminder" || isoDate(new Date(r.dueAt)) !== iso) continue;
    const time = localTime(r.dueAt);
    rows.push({ sort: `1${time}`, row: { kind: "reminder", key: `r-${r.id}`, reminder: r, time } });
  }
  return rows.sort((a, b) => a.sort.localeCompare(b.sort)).map((x) => x.row);
}

/** «через 25 мин», «через 1 ч 10 мин». */
export function inLabel(minutes: number): string {
  if (minutes < 60) return `через ${Math.max(1, minutes)} мин`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? `через ${h} ч ${m} мин` : `через ${h} ч`;
}

// Reminders already shown, by block and phase, so a restart or a reload
// never repeats one. A moved block (new start) reminds again.
const SEEN_KEY = "jarvis.agenda.seen";
const seen = new Set<string>(readSeen());

function readSeen(): string[] {
  try {
    return JSON.parse(localStorage.getItem(SEEN_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}

function keep(key: string) {
  seen.add(key);
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify([...seen].slice(-80)));
  } catch {
    /* this session only */
  }
}

const keyOf = (u: Upcoming, phase: "soon" | "now" = u.started ? "now" : "soon") => `${u.event.id}@${u.event.start}#${phase}`;

/** Mark a reminder as shown (QA: a count-down without the card). */
export function remember(u: Upcoming) {
  keep(keyOf(u));
}

interface AgendaStore {
  events: CalEvent[];
  /** A block inside the reminder window: the compact island counts down to it. */
  soon: Upcoming | null;
  /** The reminder card, until acted on, put away or timed out. */
  card: Upcoming | null;
  load: () => Promise<void>;
  tick: (now?: Date) => void;
  /** «Позже»: no more cards for this block. */
  dismiss: () => void;
  /** Timed out: the compact count-down stays. */
  hide: () => void;
}

export const useAgenda = create<AgendaStore>((set, get) => ({
  events: [],
  soon: null,
  card: null,

  load: async () => {
    if (!isTauri()) return;
    const today = new Date();
    try {
      const events = await invoke<CalEvent[]>("planner_events", { from: isoDate(addDays(today, -7)), to: isoDate(addDays(today, 29)) });
      set({ events });
      get().tick();
    } catch {
      /* the island keeps the last list */
    }
  },

  tick: (now = new Date()) => {
    const u = upcoming(get().events, now, usePrefs.getState().islandRemind);
    const card = get().card;
    let next: Upcoming | null = card && u && card.event.id === u.event.id && card.started === u.started ? u : null;
    if (u && !seen.has(keyOf(u))) {
      keep(keyOf(u));
      next = u;
    }
    set({ soon: u && !u.started ? u : null, card: next });
  },

  dismiss: () => {
    const card = get().card;
    if (card) {
      keep(keyOf(card, "soon"));
      keep(keyOf(card, "now"));
    }
    set({ card: null });
  },

  hide: () => set({ card: null }),
}));

let fed = false;

export async function startAgendaFeed() {
  if (fed) return;
  fed = true;
  window.setInterval(() => useAgenda.getState().tick(), 15_000);
  if (!isTauri()) return;
  await useAgenda.getState().load();
  await listen("planner:changed", () => void useAgenda.getState().load());
  // The window moves with the days; Google sync announces its own changes.
  window.setInterval(() => void useAgenda.getState().load(), 10 * 60_000);
}
