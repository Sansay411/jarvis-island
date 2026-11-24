import { describe, expect, it } from "vitest";
import { formatNextDue, scheduledJobs, type Reminder } from "./reminders";

const base = (over: Partial<Reminder>): Reminder => ({
  id: 1,
  kind: "reminder",
  text: "Тест",
  dueAt: "2026-11-24T12:00:00Z",
  createdAt: "2026-11-24T08:00:00Z",
  firedAt: null,
  done: false,
  ...over,
});

describe("reminders helpers", () => {
  it("filters and sorts active repeating scheduled actions", () => {
    const list: Reminder[] = [
      base({ id: 1, kind: "reminder", repeatLabel: "по будням в 09:00" }),
      base({ id: 2, kind: "action", text: "разовое", dueAt: "2026-11-24T10:00:00Z" }),
      base({ id: 3, kind: "action", text: "позже", dueAt: "2026-11-25T08:00:00Z", repeatLabel: "каждый день в 08:00" }),
      base({ id: 4, kind: "action", text: "раньше", dueAt: "2026-11-24T18:00:00Z", repeatLabel: "каждый день в 18:00" }),
      base({ id: 5, kind: "action", text: "завершено", done: true, repeatLabel: "каждый день в 07:00" }),
    ];
    expect(scheduledJobs(list).map((r) => r.id)).toEqual([4, 3]);
  });

  it("formats next due time relative to now", () => {
    const now = new Date(2026, 10, 24, 13, 0, 0);
    expect(formatNextDue(new Date(2026, 10, 24, 18, 30).toISOString(), now)).toBe("сегодня, 18:30");
    expect(formatNextDue(new Date(2026, 10, 25, 9, 5).toISOString(), now)).toBe("завтра, 09:05");
    expect(formatNextDue(new Date(2026, 10, 30, 10, 0).toISOString(), now)).toBe("30.11, 10:00");
  });
});
