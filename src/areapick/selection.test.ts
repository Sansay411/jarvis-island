import { describe, expect, it } from "vitest";
import {
  hitTestWindow,
  isClickGesture,
  normalizeLogicalRect,
  physicalSizeOf,
  sizeLabelPosition,
  type PickerWindow,
} from "./selection";

describe("area picker selection geometry", () => {
  it("normalizes drags in every direction and clamps to viewport", () => {
    expect(normalizeLogicalRect(400, 300, 120, 80, 1280, 720)).toEqual({
      x: 120,
      y: 80,
      width: 280,
      height: 220,
    });
    expect(normalizeLogicalRect(-25, -10, 1400, 900, 1280, 720)).toEqual({
      x: 0,
      y: 0,
      width: 1280,
      height: 720,
    });
  });

  it("distinguishes single clicks from drags", () => {
    expect(isClickGesture(200, 200, 202, 203)).toBe(true);
    expect(isClickGesture(200, 200, 210, 200)).toBe(false);
    expect(isClickGesture(200, 200, 200, 190)).toBe(false);
  });

  it("scales logical dimensions to physical pixels across DPI factors", () => {
    expect(physicalSizeOf({ x: 0, y: 0, width: 400, height: 200 }, 1.5)).toEqual({ width: 600, height: 300 });
    expect(physicalSizeOf({ x: 0, y: 0, width: 320, height: 180 }, 1.25)).toEqual({ width: 400, height: 225 });
    expect(physicalSizeOf({ x: 0, y: 0, width: 320, height: 180 }, 0)).toEqual({ width: 320, height: 180 });
  });

  it("picks the topmost overlapping window in Z-order", () => {
    const windows: PickerWindow[] = [
      {
        hwnd: 1,
        title: "Terminal",
        process: "wt.exe",
        rect: { x: 100, y: 100, width: 400, height: 300 },
        localX: 100,
        localY: 100,
        localWidth: 400,
        localHeight: 300,
      },
      {
        hwnd: 2,
        title: "VS Code",
        process: "Code.exe",
        rect: { x: 0, y: 0, width: 1200, height: 800 },
        localX: 0,
        localY: 0,
        localWidth: 1200,
        localHeight: 800,
      },
    ];
    expect(hitTestWindow(200, 200, windows)?.hwnd).toBe(1);
    expect(hitTestWindow(800, 600, windows)?.hwnd).toBe(2);
    expect(hitTestWindow(1250, 10, windows)).toBeNull();
  });

  it("keeps the size label inside viewport edges", () => {
    // Enough room above -> placed above selection.
    expect(sizeLabelPosition({ x: 200, y: 120, width: 300, height: 180 }, 1280, 720)).toEqual({ left: 200, top: 86 });
    // Touching top edge -> placed below selection.
    expect(sizeLabelPosition({ x: 200, y: 4, width: 300, height: 180 }, 1280, 720)).toEqual({ left: 200, top: 192 });
    // Fullscreen selection -> tucked inside top-left.
    expect(sizeLabelPosition({ x: 0, y: 0, width: 1280, height: 720 }, 1280, 720)).toEqual({ left: 8, top: 8 });
    // Near right edge -> clamped horizontally.
    const pos = sizeLabelPosition({ x: 1240, y: 200, width: 40, height: 40 }, 1280, 720, 104, 26);
    expect(pos.left).toBe(1168);
  });
});
