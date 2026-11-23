import { describe, expect, it } from "vitest";
import { daysRu, minutesRu, momentOf, type MentorState } from "./mentor";

const base: MentorState = {
  guard: true,
  xpTotal: 280,
  level: 2,
  levelName: "Ученик",
  levelFrom: 100,
  levelTo: 300,
  todayXp: 40,
  goalXp: 50,
  streak: 4,
  bestStreak: 4,
  short: { usedSec: 0, limitSec: 600 },
  youtube: { usedSec: 0, limitSec: 1800 },
  earnedSec: 0,
  strict: null,
  watching: null,
  closesToday: 0,
  focusMinToday: 0,
  week: [{ date: "2026-11-23", xp: 40, shortMin: 0, focusMin: 0, growth: false }],
  ifThen: "",
  tracks: [],
};

describe("growth moments", () => {
  it("celebrates a new level first, then a counted day, else just the XP", () => {
    expect(momentOf(null, base)).toEqual({ moment: null, gain: 0 });
    const level = momentOf(base, { ...base, xpTotal: 305, level: 3, levelName: "Практик", todayXp: 65, streak: 5 }, 1);
    expect(level.moment).toEqual({ kind: "level", level: 3, name: "Практик", xp: 25, at: 1 });
    const day = momentOf(base, { ...base, xpTotal: 292, todayXp: 52, streak: 5 }, 1);
    expect(day.moment).toEqual({ kind: "day", streak: 5, best: true, xp: 12, at: 1 });
    expect(momentOf(base, { ...base, xpTotal: 290, todayXp: 50, streak: 4, bestStreak: 9 }).moment?.kind).toBe("day");
    expect(momentOf(base, { ...base, xpTotal: 290, todayXp: 50, streak: 4, bestStreak: 9 }).moment).toMatchObject({ best: false });
    expect(momentOf(base, { ...base, xpTotal: 285, todayXp: 45 })).toEqual({ moment: null, gain: 5 });
  });

  it("stays quiet when XP goes down or the day turns over", () => {
    expect(momentOf(base, { ...base, xpTotal: 270, todayXp: 30 })).toEqual({ moment: null, gain: 0 });
    const tomorrow = { ...base, todayXp: 60, week: [{ date: "2026-11-24", xp: 60, shortMin: 0, focusMin: 0, growth: true }] };
    expect(momentOf(base, tomorrow).moment).toBeNull();
  });

  it("speaks Russian plurals", () => {
    expect([1, 2, 5, 11, 21, 22, 25].map(minutesRu)).toEqual(["1 минута", "2 минуты", "5 минут", "11 минут", "21 минута", "22 минуты", "25 минут"]);
    expect([1, 3, 5, 12].map(daysRu)).toEqual(["1 день", "3 дня", "5 дней", "12 дней"]);
  });
});
