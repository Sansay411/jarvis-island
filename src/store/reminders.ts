// Timers and reminders mirrored from the native scheduler. Alerts (fired,
// not dismissed) take the island until «Готово» or «Позже».

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

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

export function useAlerts(): Reminder[] {
  const list = useReminders((s) => s.list);
  return list.filter((r) => r.firedAt && !r.done);
}

/** The nearest running timer (not yet fired). */
export function useNextTimer(): Reminder | null {
  const list = useReminders((s) => s.list);
  return list.filter((r) => r.kind === "timer" && !r.firedAt).sort((a, b) => a.dueAt.localeCompare(b.dueAt))[0] ?? null;
}

/** «4:59», «1:02:03»: time left, ticking each second while mounted. */
export function useCountdown(dueAt: string | null): string {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!dueAt) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [dueAt]);
  if (!dueAt) return "";
  const left = Math.max(0, Math.ceil((Date.parse(dueAt) - Date.now()) / 1000));
  const h = Math.floor(left / 3600);
  const m = Math.floor((left % 3600) / 60);
  const s = String(left % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

export function localTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
}
