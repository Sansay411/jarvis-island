import { describe, expect, it } from "vitest";
import { niceScale } from "./scale";

describe("niceScale", () => {
  it("covers the data on round steps", () => {
    expect(niceScale(12, 34)).toEqual({ min: 0, max: 40, ticks: [0, 20, 40] });
    expect(niceScale(102, 118).ticks).toEqual([100, 110, 120]);
    expect(niceScale(-7, 13)).toEqual({ min: -10, max: 20, ticks: [-10, 0, 10, 20] });
  });

  it("survives a flat or broken series", () => {
    expect(niceScale(5, 5).ticks.length).toBeGreaterThanOrEqual(2);
    expect(niceScale(0, 0)).toMatchObject({ min: 0 });
    expect(niceScale(Number.NaN, 3)).toEqual({ min: 0, max: 1, ticks: [0, 1] });
  });
});
