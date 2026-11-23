import { describe, expect, it } from "vitest";
import { isBanner, layoutBanners, layoutDay } from "./layout";
import { addDays, dayDiff, durationLabel, gmtLabel, isoDate, isoDateTime, parseLocal, periodTitle, snap, spanLabel, startOfWeek, step, viewRange } from "./dates";

const ev = (start: string, end: string, allDay = false) => ({ start, end, allDay });
const day = new Date(2026, 10, 23);

describe("dates", () => {
  it("parses and formats local wall time", () => {
    const d = parseLocal("2026-11-23T15:45");
    expect(isoDateTime(d)).toBe("2026-11-23T15:45");
    expect(isoDate(parseLocal("2026-02-03"))).toBe("2026-02-03");
  });

  it("weeks start on Monday and views know their ranges", () => {
    // 2026-11-29 is a Sunday.
    expect(isoDate(startOfWeek(new Date(2026, 10, 29)))).toBe("2026-11-23");
    expect(isoDate(startOfWeek(day))).toBe("2026-11-23");
    const month = viewRange("month", new Date(2026, 10, 15));
    expect(isoDate(month.start)).toBe("2026-10-26");
    expect(month.days).toBe(42);
    expect(isoDate(step("month", new Date(2026, 11, 31), 1))).toBe("2027-01-01");
    expect(dayDiff(new Date(2026, 2, 28), new Date(2026, 2, 30))).toBe(2);
  });

  it("titles like Google", () => {
    expect(periodTitle("week", day)).toBe("Ноябрь 2026");
    expect(periodTitle("week", new Date(2026, 11, 30))).toBe("Декабрь 2026 – январь 2027");
    expect(periodTitle("week", new Date(2026, 10, 30))).toBe("Ноябрь – декабрь 2026");
    expect(periodTitle("month", day)).toBe("Ноябрь 2026");
    expect(spanLabel("2026-11-23T15:00", "2026-11-23T16:30", false)).toBe("Понедельник, 23 ноября · 15:00 – 16:30");
    expect(spanLabel("2026-11-23", "2026-11-24", true)).toBe("Понедельник, 23 ноября");
    expect(spanLabel("2026-11-23", "2026-11-26", true)).toBe("Понедельник, 23 ноября – Среда, 25 ноября");
    expect(durationLabel(90)).toBe("1 ч 30 мин");
    expect(snap(52)).toBe(45);
    expect(snap(53)).toBe(60);
    expect(gmtLabel()).toMatch(/^GMT[+-]\d\d(:\d\d)?$/);
  });
});

describe("layoutDay", () => {
  it("lays a lone event over the full width", () => {
    const [p] = layoutDay([ev("2026-11-23T10:00", "2026-11-23T11:30")], day);
    expect(p).toMatchObject({ top: 600, height: 90, left: 0, width: 1 });
  });

  it("splits overlapping events into columns and widens into free ones", () => {
    const a = ev("2026-11-23T09:00", "2026-11-23T12:00");
    const b = ev("2026-11-23T09:30", "2026-11-23T10:30");
    const c = ev("2026-11-23T11:00", "2026-11-23T11:30");
    const d = ev("2026-11-23T09:45", "2026-11-23T10:15");
    const out = layoutDay([a, b, c, d], day);
    const of = (x: object) => out.find((p) => p.item === x)!;
    // a runs on under b (which starts 30 min later); b and d sit side by side.
    expect(of(a).left).toBe(0);
    expect(of(a).width).toBeCloseTo((1 / 3) * 1.7);
    expect(of(b)).toMatchObject({ left: 1 / 3, width: 1 / 3 });
    expect(of(d)).toMatchObject({ left: 2 / 3, width: 1 / 3 });
    // c starts after b and d ended: it takes both free columns.
    expect(of(c)).toMatchObject({ left: 1 / 3, width: 2 / 3 });
    // Later starts are painted over earlier ones.
    expect(of(b).z).toBeGreaterThan(of(a).z);
  });

  it("events starting together stay side by side", () => {
    const a = ev("2026-11-23T15:00", "2026-11-23T16:00");
    const b = ev("2026-11-23T15:00", "2026-11-23T15:45");
    const out = layoutDay([a, b], day);
    expect(out.map((p) => p.width)).toEqual([0.5, 0.5]);
  });

  it("separate clusters keep full width", () => {
    const out = layoutDay([ev("2026-11-23T09:00", "2026-11-23T10:00"), ev("2026-11-23T10:00", "2026-11-23T11:00")], day);
    expect(out.map((p) => p.width)).toEqual([1, 1]);
  });

  it("clips events crossing midnight and skips banners", () => {
    const night = ev("2026-11-22T23:00", "2026-11-23T01:00");
    const out = layoutDay([night, ev("2026-11-23", "2026-11-24", true)], day);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({ top: 0, height: 60, fromBefore: true, intoNext: false });
    expect(isBanner(ev("2026-11-23T10:00", "2026-11-24T10:00"))).toBe(true);
  });
});

describe("layoutBanners", () => {
  it("stacks multi-day bars into rows across the week", () => {
    const week = startOfWeek(day);
    const trip = ev("2026-11-24", "2026-11-27", true);
    const one = ev("2026-11-25", "2026-11-26", true);
    const other = ev("2026-11-28", "2026-11-29", true);
    const out = layoutBanners([one, trip, other], week, 7);
    const of = (x: object) => out.find((p) => p.item === x)!;
    expect(of(trip)).toMatchObject({ from: 1, to: 3, row: 0 });
    expect(of(one)).toMatchObject({ from: 2, to: 2, row: 1 });
    expect(of(other)).toMatchObject({ from: 5, to: 5, row: 0 });
  });

  it("clips bars to the visible range", () => {
    const week = startOfWeek(day);
    const long = ev("2026-11-20", "2026-12-02", true);
    const [p] = layoutBanners([long], week, 7);
    expect(p).toMatchObject({ from: 0, to: 6, clippedStart: true, clippedEnd: true });
    expect(layoutBanners([ev("2026-11-30", "2026-12-01", true)], week, 7)).toHaveLength(0);
    expect(layoutBanners([ev("2026-11-22", "2026-11-23", true)], addDays(week, 0), 7)).toHaveLength(0);
  });
});
