// Notch Hub store: persistent Tasks & Week Agenda, Focus Session Engine,
// Holding Shelf (Tray), Real Activity Streak Heatmap, Routines, Mission Plan,
// and Full-Control Autopilot quick-toggle.
//
// Zero fake data in production: all initial collections start empty and populate
// strictly from SQLite/Win32/Git via `ipc.hub`. When running in standalone Vite
// browser (`!isTauri()`), actions mutate local state cleanly so QA harnesses
// remain 100% interactive without console errors.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ipc, isTauri, on } from "../lib/ipc";
import type {
  ActivityHeatmap,
  FocusState,
  MissionPlanStep,
  RoutineItem,
  RoutineSaveInput,
  ShelfItem,
  TaskColor,
  TaskItem,
} from "../lib/types";

export const EMPTY_FOCUS: FocusState = {
  active: false,
  paused: false,
  taskId: null,
  title: "",
  totalSec: 1500,
  remainingSec: 1500,
  startedAt: null,
  endsAt: null,
  completedToday: 0,
  minutesToday: 0,
};

interface HubStore {
  tasks: TaskItem[];
  focus: FocusState;
  shelf: ShelfItem[];
  activity: ActivityHeatmap | null;
  routines: RoutineItem[];
  missionSteps: MissionPlanStep[];
  fullControl: boolean;

  refreshAll: () => Promise<void>;
  refreshActivity: () => Promise<void>;

  taskAdd: (
    title: string,
    opts?: { scheduledDate?: string | null; scheduledTime?: string | null; color?: TaskColor | string | null },
  ) => Promise<TaskItem | null>;
  taskToggle: (id: string) => Promise<void>;
  taskRemove: (id: string) => Promise<void>;

  focusStart: (title: string, minutes: number, taskId?: string | null) => Promise<void>;
  focusPauseToggle: () => Promise<void>;
  focusComplete: () => Promise<void>;
  focusCancel: () => Promise<void>;

  shelfAddFiles: (paths: string[]) => Promise<void>;
  shelfAddText: (text: string) => Promise<void>;
  shelfRemove: (id: string) => Promise<void>;
  shelfClear: () => Promise<void>;

  routineSave: (input: RoutineSaveInput) => Promise<RoutineItem | null>;
  routineRemove: (id: string) => Promise<void>;
  routineRun: (id: string) => Promise<void>;

  setFullControl: (enabled: boolean) => Promise<void>;
  toggleFullControl: () => Promise<void>;
}

let localSeq = 1;
const nextLocalId = (prefix: string) => `${prefix}-${Date.now().toString(36)}-${(localSeq++).toString(36)}`;

let browserFocusTicker: number | undefined;

function syncBrowserFocusTicker() {
  if (isTauri()) return;
  window.clearInterval(browserFocusTicker);
  const f = useHub.getState().focus;
  if (!f.active || f.paused) return;
  browserFocusTicker = window.setInterval(() => {
    const cur = useHub.getState().focus;
    if (!cur.active || cur.paused) {
      window.clearInterval(browserFocusTicker);
      return;
    }
    if (cur.remainingSec <= 1) {
      window.clearInterval(browserFocusTicker);
      const mins = Math.max(1, Math.round(cur.totalSec / 60));
      useHub.setState({
        focus: {
          ...EMPTY_FOCUS,
          completedToday: cur.completedToday + 1,
          minutesToday: cur.minutesToday + mins,
        },
      });
    } else {
      useHub.setState({ focus: { ...cur, remainingSec: cur.remainingSec - 1 } });
    }
  }, 1000);
}

/** Extract optional trailing `HH:MM` from quick-entry text like `"Созвон с командой 15:30"`. */
export function parseInlineTaskTime(raw: string): { title: string; time: string | null } {
  const trimmed = raw.trim();
  const match = trimmed.match(/^(.*?)\s+([01]?\d|2[0-3]):([0-5]\d)$/);
  if (match && match[1]?.trim()) {
    const hh = match[2]!.padStart(2, "0");
    const mm = match[3]!;
    return { title: match[1].trim(), time: `${hh}:${mm}` };
  }
  return { title: trimmed, time: null };
}

export function localDateIso(d: Date = new Date()): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export const useHub = create<HubStore>((set, get) => ({
  tasks: [],
  focus: EMPTY_FOCUS,
  shelf: [],
  activity: null,
  routines: [],
  missionSteps: [],
  fullControl: false,

  refreshAll: async () => {
    if (!isTauri()) return;
    const [tasks, focus, shelf, activity, routines, settings] = await Promise.allSettled([
      ipc.hub.tasksList(),
      ipc.hub.focusGet(),
      ipc.hub.shelfList(),
      ipc.hub.activityGet(),
      ipc.hub.routinesList(),
      invoke<{ settings?: { agent?: { fullControl?: boolean } } }>("settings_get"),
    ]);
    set((s) => ({
      tasks: tasks.status === "fulfilled" ? tasks.value : s.tasks,
      focus: focus.status === "fulfilled" ? focus.value : s.focus,
      shelf: shelf.status === "fulfilled" ? shelf.value : s.shelf,
      activity: activity.status === "fulfilled" ? activity.value : s.activity,
      routines: routines.status === "fulfilled" ? routines.value : s.routines,
      fullControl:
        settings.status === "fulfilled"
          ? Boolean(settings.value?.settings?.agent?.fullControl)
          : s.fullControl,
    }));
  },

  refreshActivity: async () => {
    if (!isTauri()) return;
    try {
      set({ activity: await ipc.hub.activityGet() });
    } catch {
      /* ignore */
    }
  },

  taskAdd: async (rawTitle, opts) => {
    const parsed = parseInlineTaskTime(rawTitle);
    const title = parsed.title;
    if (!title) return null;
    const scheduledTime = opts?.scheduledTime ?? parsed.time;
    const scheduledDate = opts?.scheduledDate !== undefined ? opts.scheduledDate : localDateIso();
    const color = opts?.color ?? "blue";

    if (!isTauri()) {
      const item: TaskItem = {
        id: nextLocalId("task"),
        title,
        notes: "",
        scheduledDate,
        scheduledTime: scheduledTime ?? null,
        color,
        completed: false,
        completedAt: null,
        createdAt: new Date().toISOString(),
      };
      set((s) => ({ tasks: [...s.tasks, item] }));
      return item;
    }
    try {
      const created = await ipc.hub.taskAdd(title, { scheduledDate, scheduledTime, color });
      set((s) => ({
        tasks: s.tasks.some((t) => t.id === created.id) ? s.tasks : [...s.tasks, created],
      }));
      return created;
    } catch {
      return null;
    }
  },

  taskToggle: async (id) => {
    if (!isTauri()) {
      set((s) => ({
        tasks: s.tasks.map((t) =>
          t.id === id
            ? {
                ...t,
                completed: !t.completed,
                completedAt: !t.completed ? new Date().toISOString() : null,
              }
            : t,
        ),
      }));
      return;
    }
    // Optimistic flip backed by native event/response.
    set((s) => ({
      tasks: s.tasks.map((t) => (t.id === id ? { ...t, completed: !t.completed } : t)),
    }));
    try {
      const updated = await ipc.hub.taskToggle(id);
      set((s) => ({ tasks: s.tasks.map((t) => (t.id === id ? updated : t)) }));
      void get().refreshActivity();
    } catch {
      void get().refreshAll();
    }
  },

  taskRemove: async (id) => {
    set((s) => ({ tasks: s.tasks.filter((t) => t.id !== id) }));
    if (!isTauri()) return;
    try {
      await ipc.hub.taskRemove(id);
    } catch {
      void get().refreshAll();
    }
  },

  focusStart: async (title, minutes, taskId) => {
    const cleanTitle = title.trim() || "Глубокая работа";
    const mins = Math.max(1, Math.round(minutes));
    if (!isTauri()) {
      const now = Date.now();
      const totalSec = mins * 60;
      set((s) => ({
        focus: {
          active: true,
          paused: false,
          taskId: taskId ?? null,
          title: cleanTitle,
          totalSec,
          remainingSec: totalSec,
          startedAt: new Date(now).toISOString(),
          endsAt: new Date(now + totalSec * 1000).toISOString(),
          completedToday: s.focus.completedToday,
          minutesToday: s.focus.minutesToday,
        },
      }));
      syncBrowserFocusTicker();
      return;
    }
    try {
      set({ focus: await ipc.hub.focusStart(cleanTitle, mins, taskId) });
    } catch {
      /* ignore */
    }
  },

  focusPauseToggle: async () => {
    if (!isTauri()) {
      set((s) => {
        if (!s.focus.active) return s;
        const paused = !s.focus.paused;
        const endsAt = paused ? null : new Date(Date.now() + s.focus.remainingSec * 1000).toISOString();
        return { focus: { ...s.focus, paused, endsAt } };
      });
      syncBrowserFocusTicker();
      return;
    }
    try {
      set({ focus: await ipc.hub.focusPauseToggle() });
    } catch {
      /* ignore */
    }
  },

  focusComplete: async () => {
    if (!isTauri()) {
      const cur = get().focus;
      const elapsedMin = Math.max(1, Math.round((cur.totalSec - cur.remainingSec) / 60) || Math.round(cur.totalSec / 60));
      set((s) => ({
        focus: {
          ...EMPTY_FOCUS,
          completedToday: cur.completedToday + 1,
          minutesToday: cur.minutesToday + elapsedMin,
        },
        tasks: cur.taskId
          ? s.tasks.map((t) => (t.id === cur.taskId ? { ...t, completed: true, completedAt: new Date().toISOString() } : t))
          : s.tasks,
      }));
      syncBrowserFocusTicker();
      return;
    }
    try {
      set({ focus: await ipc.hub.focusComplete() });
      void get().refreshActivity();
    } catch {
      /* ignore */
    }
  },

  focusCancel: async () => {
    if (!isTauri()) {
      set((s) => ({
        focus: {
          ...EMPTY_FOCUS,
          completedToday: s.focus.completedToday,
          minutesToday: s.focus.minutesToday,
        },
      }));
      syncBrowserFocusTicker();
      return;
    }
    try {
      set({ focus: await ipc.hub.focusCancel() });
    } catch {
      /* ignore */
    }
  },

  shelfAddFiles: async (paths) => {
    if (paths.length === 0) return;
    if (!isTauri()) {
      const added: ShelfItem[] = paths.map((p) => {
        const label = p.split(/[\\/]/).pop() ?? p;
        return {
          id: nextLocalId("shelf"),
          kind: "file",
          label,
          value: p,
          subtitle: p,
          sizeBytes: null,
          exists: true,
          pinned: false,
          createdAt: new Date().toISOString(),
        };
      });
      set((s) => ({ shelf: [...added, ...s.shelf] }));
      return;
    }
    try {
      const list = await ipc.hub.shelfAddFiles(paths);
      set({ shelf: list });
    } catch {
      void get().refreshAll();
    }
  },

  shelfAddText: async (raw) => {
    const text = raw.trim();
    if (!text) return;
    const isUrl = /^https?:\/\/\S+$/i.test(text);
    if (!isTauri()) {
      const item: ShelfItem = {
        id: nextLocalId("shelf"),
        kind: isUrl ? "url" : "text",
        label: isUrl ? text.replace(/^https?:\/\//i, "").slice(0, 48) : text.slice(0, 56),
        value: text,
        subtitle: isUrl ? "Веб-ссылка" : `${text.length} симв.`,
        sizeBytes: null,
        exists: true,
        pinned: false,
        createdAt: new Date().toISOString(),
      };
      set((s) => ({ shelf: [item, ...s.shelf] }));
      return;
    }
    try {
      const item = await ipc.hub.shelfAddText(text);
      set((s) => ({
        shelf: s.shelf.some((x) => x.id === item.id) ? s.shelf : [item, ...s.shelf],
      }));
    } catch {
      void get().refreshAll();
    }
  },

  shelfRemove: async (id) => {
    set((s) => ({ shelf: s.shelf.filter((x) => x.id !== id) }));
    if (!isTauri()) return;
    try {
      await ipc.hub.shelfRemove(id);
    } catch {
      void get().refreshAll();
    }
  },

  shelfClear: async () => {
    set({ shelf: [] });
    if (!isTauri()) return;
    try {
      await ipc.hub.shelfClear();
    } catch {
      void get().refreshAll();
    }
  },

  routineSave: async (input) => {
    if (!isTauri()) {
      const item: RoutineItem = {
        id: input.id ?? nextLocalId("routine"),
        title: input.title,
        subtitle: input.subtitle,
        icon: input.icon,
        prompt: input.prompt,
        autonomy: input.autonomy,
      };
      set((s) => ({
        routines: s.routines.some((r) => r.id === item.id)
          ? s.routines.map((r) => (r.id === item.id ? item : r))
          : [...s.routines, item],
      }));
      return item;
    }
    try {
      const saved = await ipc.hub.routineSave(input);
      return saved;
    } catch {
      return null;
    }
  },

  routineRemove: async (id) => {
    set((s) => ({ routines: s.routines.filter((r) => r.id !== id) }));
    if (!isTauri()) return;
    try {
      await ipc.hub.routineRemove(id);
    } catch {
      void get().refreshAll();
    }
  },

  routineRun: async (id) => {
    if (!isTauri()) return;
    try {
      await ipc.hub.routineRun(id);
    } catch {
      /* ignore */
    }
  },

  setFullControl: async (enabled) => {
    set({ fullControl: enabled });
    if (!isTauri()) return;
    try {
      const res = await ipc.hub.setFullControl(enabled);
      set({ fullControl: res });
    } catch {
      /* revert if rejected */
      void get().refreshAll();
    }
  },

  toggleFullControl: async () => {
    await get().setFullControl(!get().fullControl);
  },
}));

let hubStarted = false;

export async function startHubFeed() {
  if (hubStarted) return;
  hubStarted = true;
  if (!isTauri()) return;

  await on("hub:tasks", (tasks) => useHub.setState({ tasks }));
  await on("hub:focus", (focus) => useHub.setState({ focus }));
  await on("hub:shelf", (shelf) => useHub.setState({ shelf }));
  await on("hub:routines", (routines) => useHub.setState({ routines }));

  await on("settings:changed", async () => {
    try {
      const v = await invoke<{ settings?: { agent?: { fullControl?: boolean } } }>("settings_get");
      useHub.setState({ fullControl: Boolean(v?.settings?.agent?.fullControl) });
    } catch {
      /* ignore */
    }
  });

  // Listen for live multi-step executive plans published by `mission_plan` tool
  // (accepting both camelCase `planUpdated` and snake_case `plan_updated`).
  await listen<{ type?: string; steps?: MissionPlanStep[] }>("agent:event", (e) => {
    const p = e.payload;
    if (!p || typeof p !== "object") return;
    if ((p.type === "planUpdated" || p.type === "plan_updated") && Array.isArray(p.steps)) {
      useHub.setState({ missionSteps: p.steps });
    } else if (p.type === "turnStarted" || p.type === "turn_started") {
      useHub.setState({ missionSteps: [] });
    } else if (p.type === "turnFinished" || p.type === "turn_finished") {
      // Keep completed plan visible briefly, then clear so compact/peek stays clean.
      window.setTimeout(() => {
        useHub.setState({ missionSteps: [] });
      }, 2800);
    }
  });

  await useHub.getState().refreshAll();
}
