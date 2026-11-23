import { describe, expect, it } from "vitest";
import { dayRows, eventsOn, focusBlock, inLabel, nowAndNext, upcoming } from "./agenda";
import type { CalEvent } from "./planner";
import type { TaskItem } from "../lib/types";
import type { Reminder } from "./reminders";

let n = 1;
const ev = (start: string, end: string, title: string, extra: Partial<CalEvent> = {}): CalEvent => ({
  id: `e${n++}`,
  title,
  notes: "",
  location: "",
  start,
  end,
  allDay: start.length === 10,
  color: "",
  category: "work",
  done: false,
  seriesId: null,
  googleId: null,
  recurring: false,
  updatedAt: "",
  ...extra,
});

const now = new Date(2026, 10, 23, 14, 20);
const events = [
  ev("2026-11-23T09:00", "2026-11-23T10:30", "Глубокая работа"),
  ev("2026-11-23T14:00", "2026-11-23T15:00", "Ревью"),
  ev("2026-11-23T14:28", "2026-11-23T16:00", "Встреча с заказчиком", { category: "personal" }),
  ev("2026-11-23T18:30", "2026-11-23T19:30", "Зал", { category: "sport" }),
  ev("2026-11-23", "2026-11-25", "Командировка"),
  ev("2026-11-24T10:00", "2026-11-24T11:00", "Завтра"),
];

describe("island agenda", () => {
  it("lists a day with all-day events first and multi-day ones on every day", () => {
    expect(eventsOn(events, "2026-11-23").map((e) => e.title)).toEqual(["Командировка", "Глубокая работа", "Ревью", "Встреча с заказчиком", "Зал"]);
    expect(eventsOn(events, "2026-11-24").map((e) => e.title)).toEqual(["Командировка", "Завтра"]);
    expect(eventsOn(events, "2026-11-25")).toEqual([]);
  });

  it("knows what is on now and what is next", () => {
    const { current, next } = nowAndNext(events, now);
    expect(current?.title).toBe("Ревью");
    expect(next?.title).toBe("Встреча с заказчиком");
    // A block marked done is not «now».
    const done = events.map((e) => (e.title === "Ревью" ? { ...e, done: true } : e));
    expect(nowAndNext(done, now).current).toBeNull();
  });

  it("reminds within the lead, at the start, and never about all-day or done blocks", () => {
    const soon = upcoming(events, now, 10);
    expect(soon?.event.title).toBe("Встреча с заказчиком");
    expect(soon?.minutes).toBe(8);
    expect(soon?.started).toBe(false);
    // Focus for the whole block: 14:28–16:00 → 90 min (rounded to 5).
    expect(soon?.focusMin).toBe(90);
    expect(upcoming(events, now, 5)).toBeNull();
    expect(upcoming(events, now, 0)).toBeNull();
    // A minute after the start it is still worth a card, with what is left.
    const begun = upcoming(events, new Date(2026, 10, 23, 14, 29), 10);
    expect(begun?.started).toBe(true);
    expect(begun?.minutes).toBe(0);
    expect(begun?.focusMin).toBe(90);
    expect(upcoming(events, new Date(2026, 10, 23, 14, 31), 10)).toBeNull();
    expect(upcoming([ev("2026-11-23T14:25", "2026-11-23T15:00", "x", { done: true })], now, 10)).toBeNull();
  });

  it("offers the focus deck the block on now, or a work block about to start", () => {
    // 14:20: «Ревью» (work) runs to 15:00 → 40 min.
    expect(focusBlock(events, now)).toMatchObject({ started: true, focusMin: 40 });
    expect(focusBlock(events, now)?.event.title).toBe("Ревью");
    // 17:00: nothing on; «Зал» at 18:30 is sport and too far anyway.
    expect(focusBlock(events, new Date(2026, 10, 23, 17, 0))).toBeNull();
    // 08:50: «Глубокая работа» starts in 10 min → the whole 90 min.
    expect(focusBlock(events, new Date(2026, 10, 23, 8, 50))).toMatchObject({ started: false, focusMin: 90 });
  });

  it("merges events, tasks and reminders by time", () => {
    const task = (id: string, time: string | null, completed = false): TaskItem => ({ id, title: id, notes: "", scheduledDate: "2026-11-23", scheduledTime: time, color: "blue", completed, completedAt: null, createdAt: "" });
    const reminder: Reminder = { id: 7, kind: "reminder", text: "Позвонить", dueAt: new Date(2026, 10, 23, 17, 0).toISOString(), createdAt: "", firedAt: null, done: false };
    const rows = dayRows(events, [task("без времени", null), task("отчёт", "15:30"), task("готово", "08:00", true)], [reminder], "2026-11-23", now);
    expect(rows.map((r) => r.key)).toEqual(["e-e5", "e-e1", "e-e2", "e-e3", "t-отчёт", "r-7", "e-e4", "t-без времени", "t-готово"]);
    const states = rows.flatMap((r) => (r.kind === "event" ? [r.state] : []));
    expect(states).toEqual(["allday", "past", "now", "next", "later"]);
  });

  it("says how long until", () => {
    expect(inLabel(0)).toBe("через 1 мин");
    expect(inLabel(25)).toBe("через 25 мин");
    expect(inLabel(60)).toBe("через 1 ч");
    expect(inLabel(70)).toBe("через 1 ч 10 мин");
  });
});
