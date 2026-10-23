// Timers and reminders mirrored from the native scheduler. Alerts (fired,
// not dismissed) take the island until «Готово» or «Позже».

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface Reminder {
  id: number;
  kind: "timer" | "reminder";
  text: string;
  dueAt: string;
  createdAt: string;
  firedAt: string | null;
  done: boolean;
}

export const useReminders = create<{ list: Reminder[] }>(() => ({ list: [] }));

export async function startRemindersFeed() {
  await listen<Reminder[]>("reminders:changed", (e) => useReminders.setState({ list: e.payload }));
  try {
    useReminders.setState({ list: await invoke<Reminder[]>("reminders_list") });
  } catch {
    /* the scheduler sends the list on its own as well */
  }
}

export const reminderDone = (id: number) => invoke("reminder_done", { id });
export const reminderSnooze = (id: number, minutes: number) => invoke("reminder_snooze", { id, minutes });
export const reminderExtend = (id: number, seconds: number) => invoke("reminder_extend", { id, seconds });

export function useAlerts(): Reminder[] {
  const list = useReminders((s) => s.list);
  return list.filter((r) => r.firedAt && !r.done);
}

/** Running timers (not yet fired), the one ending first first. */
export function useTimers(): Reminder[] {
  const list = useReminders((s) => s.list);
  return list.filter((r) => r.kind === "timer" && !r.firedAt).sort((a, b) => a.dueAt.localeCompare(b.dueAt));
}

/** The nearest running timer (not yet fired). */
export function useNextTimer(): Reminder | null {
  return useTimers()[0] ?? null;
}

export function localTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
}
