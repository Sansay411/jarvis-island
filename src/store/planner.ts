// The planner window's state: which period is on screen, the events loaded
// for it (plus a week either side, so paging is instant), the hub's tasks
// that sit on the calendar, Google sync status and «Спланировать день»
// proposals. Outside Tauri (visual QA) every change stays in memory.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage, isTauri } from "../lib/ipc";
import type { TaskItem } from "../lib/types";
import { addDays, isoDate, isoDateTime, parseLocal, startOfDay, step, viewRange, type ViewKind } from "../planner/dates";

export interface CalEvent {
  id: string;
  title: string;
  notes: string;
  location: string;
  start: string;
  end: string;
  allDay: boolean;
  color: string;
  category: string;
  done: boolean;
  seriesId: string | null;
  googleId: string | null;
  recurring: boolean;
  updatedAt: string;
}

export interface EventInput {
  title: string;
  notes?: string;
  location?: string;
  start: string;
  end?: string;
  allDay?: boolean;
  color?: string;
  category?: string;
  repeat?: "none" | "daily" | "weekdays" | "weekly";
}

export type EventPatch = Partial<Pick<CalEvent, "title" | "notes" | "location" | "start" | "end" | "allDay" | "color" | "category" | "done">>;

export interface Suggestion {
  title: string;
  start: string;
  end: string;
  category: string;
  why: string;
}

export interface SyncStatus {
  connected: boolean;
  account: string;
  clientId: string;
  hasSecret: boolean;
  syncing: boolean;
  connecting: boolean;
  lastSync: string | null;
  error: string | null;
}

/** Google Calendar's event colours, in its own order and Russian names. */
export const COLORS: Array<{ id: string; label: string; hex: string }> = [
  { id: "tomato", label: "Томатный", hex: "#d50000" },
  { id: "flamingo", label: "Фламинго", hex: "#e67c73" },
  { id: "tangerine", label: "Мандариновый", hex: "#f4511e" },
  { id: "banana", label: "Банановый", hex: "#f6bf26" },
  { id: "sage", label: "Шалфей", hex: "#33b679" },
  { id: "basil", label: "Базилик", hex: "#0b8043" },
  { id: "peacock", label: "Павлин", hex: "#039be5" },
  { id: "blueberry", label: "Черника", hex: "#3f51b5" },
  { id: "lavender", label: "Лаванда", hex: "#7986cb" },
  { id: "grape", label: "Виноград", hex: "#8e24aa" },
  { id: "graphite", label: "Графит", hex: "#616161" },
];

export const CATEGORIES: Array<{ id: string; label: string; color: string }> = [
  { id: "work", label: "Работа", color: "blueberry" },
  { id: "study", label: "Учёба", color: "grape" },
  { id: "sport", label: "Спорт", color: "basil" },
  { id: "project", label: "Проекты", color: "tangerine" },
  { id: "personal", label: "Личное", color: "flamingo" },
  { id: "google", label: "Google Календарь", color: "peacock" },
];

export const hexOf = (color: string) => COLORS.find((c) => c.id === color)?.hex ?? "#039be5";
export const categoryOf = (id: string) => CATEGORIES.find((c) => c.id === id) ?? CATEGORIES[4]!;
export const eventColor = (e: Pick<CalEvent, "color" | "category">) => (e.color ? e.color : categoryOf(e.category).color);
export const eventHex = (e: Pick<CalEvent, "color" | "category">) => hexOf(eventColor(e));
/** Light swatches carry dark text, like Google's banana. */
export const inkOn = (color: string) => (color === "banana" ? "#1f1f1f" : "#ffffff");

const HIDDEN_KEY = "jarvis.planner.hidden";
const VIEW_KEY = "jarvis.planner.view";

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function writeJson(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* session only */
  }
}

export interface Undo {
  label: string;
  run: () => Promise<void>;
}

interface PlannerStore {
  view: ViewKind;
  anchor: Date;
  events: CalEvent[];
  tasks: TaskItem[];
  hidden: string[];
  sync: SyncStatus | null;
  suggestions: Suggestion[];
  suggesting: boolean;
  toast: { text: string; undo?: Undo; id: number } | null;
  /** QA: freeze «now» for screenshots. */
  now: Date | null;

  setView: (v: ViewKind) => void;
  setAnchor: (d: Date) => void;
  goToday: () => void;
  stepBy: (dir: 1 | -1) => void;
  load: () => Promise<void>;
  create: (input: EventInput) => Promise<CalEvent[]>;
  update: (id: string, patch: EventPatch, opts?: { undoLabel?: string }) => Promise<CalEvent | null>;
  remove: (id: string, following?: boolean) => Promise<void>;
  toggleHidden: (category: string) => void;
  addTask: (title: string, date: string, time: string | null) => Promise<void>;
  toggleTask: (id: string) => Promise<void>;
  suggest: (date: Date) => Promise<void>;
  accept: (s: Suggestion) => Promise<void>;
  acceptAll: () => Promise<void>;
  dismissSuggestions: () => void;
  showToast: (text: string, undo?: Undo) => void;
  syncNow: () => Promise<void>;
}

let toastSeq = 1;
let localSeq = 1;

function localEvent(input: EventInput): CalEvent {
  const allDay = input.allDay ?? input.start.length === 10;
  const start = parseLocal(input.start);
  const end = input.end ? parseLocal(input.end) : allDay ? addDays(start, 1) : new Date(start.getTime() + 3600_000);
  return {
    id: `local-${Date.now().toString(36)}-${localSeq++}`,
    title: input.title.trim() || "(Без названия)",
    notes: input.notes ?? "",
    location: input.location ?? "",
    start: allDay ? isoDate(start) : isoDateTime(start),
    end: allDay ? isoDate(end) : isoDateTime(end),
    allDay,
    color: input.color ?? "",
    category: input.category ?? "personal",
    done: false,
    seriesId: null,
    googleId: null,
    recurring: false,
    updatedAt: new Date().toISOString(),
  };
}

/** The dates a view needs, with a week of slack either side. */
function loadRange(view: ViewKind, anchor: Date): { from: string; to: string } {
  const { start, days } = viewRange(view, anchor);
  return { from: isoDate(addDays(start, -7)), to: isoDate(addDays(start, days + 7)) };
}

export const usePlanner = create<PlannerStore>((set, get) => ({
  view: readJson<ViewKind>(VIEW_KEY, "week"),
  anchor: startOfDay(new Date()),
  events: [],
  tasks: [],
  hidden: readJson<string[]>(HIDDEN_KEY, []),
  sync: null,
  suggestions: [],
  suggesting: false,
  toast: null,
  now: null,

  setView: (view) => {
    writeJson(VIEW_KEY, view);
    set({ view });
    void get().load();
  },
  setAnchor: (d) => {
    set({ anchor: startOfDay(d) });
    void get().load();
  },
  goToday: () => get().setAnchor(get().now ?? new Date()),
  stepBy: (dir) => get().setAnchor(step(get().view, get().anchor, dir)),

  load: async () => {
    if (!isTauri()) return;
    const { from, to } = loadRange(get().view, get().anchor);
    try {
      const [events, tasks] = await Promise.all([
        invoke<CalEvent[]>("planner_events", { from, to }),
        invoke<TaskItem[]>("hub_tasks_list"),
      ]);
      set({ events, tasks });
    } catch (e) {
      get().showToast(errorMessage(e));
    }
  },

  create: async (input) => {
    if (!isTauri()) {
      const ev = localEvent(input);
      set((s) => ({ events: [...s.events, ev] }));
      return [ev];
    }
    try {
      const created = await invoke<CalEvent[]>("planner_create", { input });
      set((s) => ({ events: [...s.events.filter((e) => !created.some((c) => c.id === e.id)), ...created] }));
      const first = created[0];
      if (first) {
        get().showToast(created.length > 1 ? `Создано ${created.length} мероприятий` : "Мероприятие сохранено", {
          label: "Отменить",
          run: async () => {
            for (const c of created) await invoke("planner_delete", { id: c.id, following: false }).catch(() => {});
            await get().load();
          },
        });
      }
      return created;
    } catch (e) {
      get().showToast(errorMessage(e));
      return [];
    }
  },

  update: async (id, patch, opts) => {
    const before = get().events.find((e) => e.id === id);
    if (!before) return null;
    // Optimistic: the grid moves at once; a failure puts it back.
    set((s) => ({ events: s.events.map((e) => (e.id === id ? { ...e, ...patch } : e)) }));
    if (!isTauri()) return { ...before, ...patch };
    try {
      const saved = await invoke<CalEvent>("planner_update", { id, patch });
      set((s) => ({ events: s.events.map((e) => (e.id === id ? saved : e)) }));
      if (opts?.undoLabel) {
        const back: EventPatch = { start: before.start, end: before.end, allDay: before.allDay };
        get().showToast(opts.undoLabel, {
          label: "Отменить",
          run: async () => {
            await get().update(id, back);
          },
        });
      }
      return saved;
    } catch (e) {
      set((s) => ({ events: s.events.map((x) => (x.id === id ? before : x)) }));
      get().showToast(errorMessage(e));
      return null;
    }
  },

  remove: async (id, following = false) => {
    const before = get().events.find((e) => e.id === id);
    if (!before) return;
    const gone = following && before.seriesId
      ? get().events.filter((e) => e.seriesId === before.seriesId && e.start >= before.start)
      : [before];
    set((s) => ({ events: s.events.filter((e) => !gone.some((g) => g.id === e.id)) }));
    if (!isTauri()) return;
    try {
      await invoke("planner_delete", { id, following });
      get().showToast(gone.length > 1 ? `Удалено мероприятий: ${gone.length}` : "Мероприятие удалено", {
        label: "Отменить",
        run: async () => {
          for (const g of gone) {
            await invoke("planner_create", {
              input: { title: g.title, notes: g.notes, location: g.location, start: g.start, end: g.end, allDay: g.allDay, color: g.color, category: g.category },
            }).catch(() => {});
          }
          await get().load();
        },
      });
    } catch (e) {
      get().showToast(errorMessage(e));
      await get().load();
    }
  },

  toggleHidden: (category) => {
    const hidden = get().hidden.includes(category) ? get().hidden.filter((c) => c !== category) : [...get().hidden, category];
    writeJson(HIDDEN_KEY, hidden);
    set({ hidden });
  },

  addTask: async (title, date, time) => {
    if (!isTauri()) {
      const t: TaskItem = { id: `t-${localSeq++}`, title, notes: "", scheduledDate: date, scheduledTime: time, color: "blue", completed: false, completedAt: null, createdAt: new Date().toISOString() };
      set((s) => ({ tasks: [...s.tasks, t] }));
      return;
    }
    try {
      await invoke("hub_task_add", { title, scheduledDate: date, scheduledTime: time, color: null, notes: null });
      get().showToast("Задача добавлена");
    } catch (e) {
      get().showToast(errorMessage(e));
    }
  },

  toggleTask: async (id) => {
    set((s) => ({ tasks: s.tasks.map((t) => (t.id === id ? { ...t, completed: !t.completed } : t)) }));
    if (isTauri()) await invoke("hub_task_toggle", { id }).catch((e) => get().showToast(errorMessage(e)));
  },

  suggest: async (date) => {
    set({ suggesting: true, suggestions: [] });
    try {
      const suggestions = isTauri()
        ? await invoke<Suggestion[]>("planner_suggest", { date: isoDate(date) })
        : [];
      set({ suggestions });
      if (suggestions.length === 0) get().showToast("Свободного времени на этот день почти нет");
    } catch (e) {
      get().showToast(errorMessage(e));
    } finally {
      set({ suggesting: false });
    }
  },

  accept: async (s) => {
    set((st) => ({ suggestions: st.suggestions.filter((x) => x !== s) }));
    await get().create({ title: s.title, start: s.start, end: s.end, category: s.category, notes: s.why });
  },

  acceptAll: async () => {
    const all = get().suggestions;
    set({ suggestions: [] });
    for (const s of all) {
      if (isTauri()) await invoke("planner_create", { input: { title: s.title, start: s.start, end: s.end, category: s.category, notes: s.why } }).catch(() => {});
      else set((st) => ({ events: [...st.events, localEvent({ title: s.title, start: s.start, end: s.end, category: s.category })] }));
    }
    await get().load();
    get().showToast(`В календарь добавлено блоков: ${all.length}`);
  },

  dismissSuggestions: () => set({ suggestions: [] }),

  showToast: (text, undo) => {
    const id = toastSeq++;
    set({ toast: { text, undo, id } });
    window.setTimeout(() => {
      if (get().toast?.id === id) set({ toast: null });
    }, undo ? 6000 : 3500);
  },

  syncNow: async () => {
    if (!isTauri()) return;
    try {
      const sync = await invoke<SyncStatus>("planner_google_sync");
      set({ sync });
    } catch (e) {
      get().showToast(errorMessage(e));
    }
  },
}));

let fed = false;

export async function startPlannerFeed() {
  if (fed || !isTauri()) return;
  fed = true;
  await usePlanner.getState().load();
  invoke<SyncStatus>("planner_google_status").then((sync) => usePlanner.setState({ sync })).catch(() => {});
  await listen("planner:changed", () => void usePlanner.getState().load());
  await listen<TaskItem[]>("hub:tasks", (e) => usePlanner.setState({ tasks: e.payload }));
  await listen<SyncStatus>("planner:sync", (e) => usePlanner.setState({ sync: e.payload }));
  await listen<string>("planner:goto", (e) => usePlanner.getState().setAnchor(parseLocal(e.payload)));
}
