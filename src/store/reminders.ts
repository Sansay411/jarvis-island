// Timers and reminders mirrored from the native scheduler. Alerts (fired,
// not dismissed) take the island until «Готово» or «Позже».

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface RepeatRule {
  kind: "daily" | "weekdays" | "weekly" | "monthly" | "interval";
  at?: string;
  days?: string[];
  day?: number;
  everyMinutes?: number;
  from?: string;
  to?: string;
  until?: string;
  count?: number;
}

export interface Reminder {
  id: number;
  kind: "timer" | "reminder" | "action";
  text: string;
  dueAt: string;
  createdAt: string;
  firedAt: string | null;
  done: boolean;
  repeat?: RepeatRule | null;
  repeatLabel?: string | null;
  runs?: number;
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
export const reminderCancel = (id: number) => invoke("reminder_cancel", { id });
export const reminderSkip = (id: number) => invoke<string | null>("reminder_skip", { id });
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

/** Active repeating scheduled actions, nearest first. */
export function scheduledJobs(list: Reminder[]): Reminder[] {
  return list
    .filter((r) => !r.done && r.kind === "action" && Boolean(r.repeat || r.repeatLabel))
    .sort((a, b) => a.dueAt.localeCompare(b.dueAt));
}

export function useScheduledJobs(): Reminder[] {
  const list = useReminders((s) => s.list);
  return scheduledJobs(list);
}

export function localTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
}

/** Compact Russian next-run label: «сегодня, 18:00», «завтра, 09:00», «27.11, 10:00». */
export function formatNextDue(iso: string, now: Date = new Date()): string {
  const due = new Date(iso);
  if (Number.isNaN(due.getTime())) return "";
  const hm = `${String(due.getHours()).padStart(2, "0")}:${String(due.getMinutes()).padStart(2, "0")}`;
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diffDays = Math.round((startOfDay(due) - startOfDay(now)) / 86_400_000);
  if (diffDays === 0) return `сегодня, ${hm}`;
  if (diffDays === 1) return `завтра, ${hm}`;
  const dd = String(due.getDate()).padStart(2, "0");
  const mm = String(due.getMonth() + 1).padStart(2, "0");
  return `${dd}.${mm}, ${hm}`;
}
