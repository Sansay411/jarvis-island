import { describe, expect, it } from "vitest";
import { countUpValue, deckDirection, FOCUS_PRESETS, middleTruncate, waveDelay } from "./motion";

describe("deckDirection", () => {
  it("follows tab order", () => {
    expect(deckDirection("nook", "tray")).toBe(1);
    expect(deckDirection("activity", "focus")).toBe(-1);
    expect(deckDirection("tray", "tray")).toBe(0);
  });
});

describe("middleTruncate", () => {
  it("cuts the middle", () => {
    const s = middleTruncate("Bartender Materials", 16);
    expect(s).toBe("Barten…aterials");
    expect(s.length).toBeLessThanOrEqual(16);
  });
  it("keeps the extension of cyrillic names", () => {
    const s = middleTruncate("Отчёт_за_ноябрь_финальный.pdf", 14);
    expect(s.endsWith(".pdf")).toBe(true);
    expect(s.length).toBeLessThanOrEqual(14);
    expect(s).toContain("…");
  });
  it("leaves short names and handles names without extension", () => {
    expect(middleTruncate("a.txt", 16)).toBe("a.txt");
    expect(middleTruncate("README", 4).length).toBeLessThanOrEqual(4);
  });
});

describe("waveDelay", () => {
  it("staggers by week and caps", () => {
    expect(waveDelay(0, 53)).toBe(0);
    expect(waveDelay(10, 53)).toBeCloseTo(0.12);
    expect(waveDelay(200, 53)).toBe(0.65);
  });
});

describe("countUpValue", () => {
  it("eases out to the target", () => {
    expect(countUpValue(727, 0)).toBe(0);
    expect(countUpValue(727, 1)).toBe(727);
    expect(countUpValue(0, 0.5)).toBe(0);
    expect(countUpValue(727, 0.5)).toBeGreaterThan(363);
  });
});

describe("FOCUS_PRESETS", () => {
  it("matches the spec", () => {
    expect([...FOCUS_PRESETS]).toEqual([5, 15, 25, 50]);
  });
});
