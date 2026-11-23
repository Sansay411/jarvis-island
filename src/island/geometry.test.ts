import { describe, expect, it } from "vitest";
import { dimsFor, notchLayout, NOTCH_WINGS, shapeRect, type DimsContext, type NotchGeometry } from "./geometry";

const ctx: DimsContext = { hasMedia: false, hasActivity: false, jobRows: 0, lip: false, confirm: false };
const notch: NotchGeometry = { has_notch: true, notch_width: 200, notch_height: 32, menu_bar_height: 32 };

describe("notch geometry", () => {
  it("preserves Windows and missing-geometry dimensions", () => {
    expect(dimsFor("compact", ctx)).toEqual({ w: 174, h: 36 });
    expect(dimsFor("peek", ctx)).toEqual({ w: 616, h: 206 });
    expect(dimsFor("compact", { ...ctx, notch: null })).toEqual(dimsFor("compact", ctx));
  });

  it("reserves the notch exclusion and wings in every compact state", () => {
    for (const state of [{}, { hasMedia: true }, { thinking: true }, { lip: true }]) {
      const dims = dimsFor("compact", { ...ctx, ...state, notch });
      expect(dims).toEqual({ w: 200 + NOTCH_WINGS, h: 32 });
      expect(shapeRect(dims)).toEqual({ x: (760 - dims.w) / 2, y: 0, width: dims.w, height: 32 });
    }
    expect(dimsFor("compact", { ...ctx, thinking: true, notch: { ...notch, notch_width: 100 } }).w).toBe(332);
  });

  it("adds a black notch header above expanded content", () => {
    for (const mode of ["peek", "workspace", "voice", "drop", "files", "clips", "weather", "notice"] as const) {
      const base = dimsFor(mode, ctx);
      const expanded = dimsFor(mode, { ...ctx, notch });
      expect(expanded.h).toBe(base.h + 32);
      expect(expanded.w).toBeGreaterThanOrEqual(200 + NOTCH_WINGS);
    }
  });

  it("uses menu-bar height only for notchless macOS compact mode", () => {
    const mac = { ...notch, has_notch: false, notch_width: 0, notch_height: 0, menu_bar_height: 24 };
    expect(dimsFor("compact", { ...ctx, notch: mac })).toEqual({ w: 174, h: 24 });
    expect(dimsFor("peek", { ...ctx, notch: mac })).toEqual(dimsFor("peek", ctx));
  });

  it("ignores invalid or empty geometry", () => {
    for (const geometry of [undefined, null, { ...notch, notch_width: NaN }, { ...notch, notch_height: -1 }, { ...notch, notch_width: 0, notch_height: 0, menu_bar_height: 0 }]) {
      expect(notchLayout(geometry).hasNotch).toBe(false);
      expect(dimsFor("compact", { ...ctx, notch: geometry })).toEqual({ w: 174, h: 36 });
    }
  });
});
