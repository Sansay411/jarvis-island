import { describe, expect, it } from "vitest";
import { clampGain, formatDb, formatGain, meterFraction, phasePrompt, snrWord, testProgress } from "./micLevels";

describe("micLevels", () => {
  it("maps levels onto the meter and clamps the ends", () => {
    expect(meterFraction(-75)).toBe(0);
    expect(meterFraction(-5)).toBe(1);
    expect(meterFraction(-40)).toBeCloseTo(0.5);
    expect(meterFraction(-120)).toBe(0);
    expect(meterFraction(6)).toBe(1);
    expect(meterFraction(Number.NaN)).toBe(0);
  });

  it("formats decibels the Russian way", () => {
    expect(formatDb(-32.4)).toBe("−32 дБ");
    expect(formatDb(-95)).toBe("—");
    expect(formatGain(17.6)).toBe("+18 дБ");
    expect(formatGain(-4.2)).toBe("−4 дБ");
    expect(formatGain(0.3)).toBe("0 дБ");
  });

  it("keeps manual sensitivity within -6…+30 dB", () => {
    expect(clampGain(45)).toBe(30);
    expect(clampGain(-20)).toBe(-6);
    expect(clampGain(12.4)).toBe(12);
    expect(clampGain(Number.NaN)).toBe(0);
  });

  it("walks through the test phases", () => {
    expect(phasePrompt("silence")).toContain("Помолчите");
    expect(phasePrompt("speech")).toContain("Джарвис, какая сегодня погода");
    expect(testProgress("silence", 1)).toBeCloseTo(2 / 6.5);
    expect(testProgress("speech", 0.5)).toBeGreaterThan(testProgress("silence", 1));
    expect(testProgress("analyzing", 0)).toBe(1);
    expect(testProgress("idle", 0.7)).toBe(0);
  });

  it("names the signal-to-noise ratio", () => {
    expect(snrWord(30)).toBe("отлично");
    expect(snrWord(12)).toBe("терпимо");
    expect(snrWord(4)).toBe("мало");
  });
});
