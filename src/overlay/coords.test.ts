import { describe, expect, it } from "vitest";
import {
  arrowStart,
  computeArrowGeometry,
  cssToPhysicalRect,
  isPointInHitRects,
  physicalToCssPoint,
  physicalToCssRect,
  placeCaption,
  placeCard,
  rectsOverlap,
  segmentHitsRect,
} from "./coords";
import type { OverlayMonitor } from "./types";

describe("overlay coordinate mapping", () => {
  it("converts physical screen coordinates to CSS across DPI scales and negative origins", () => {
    const mon125: OverlayMonitor = { originX: -1920, originY: 120, width: 1920, height: 1080, scale: 1.25 };
    expect(physicalToCssPoint({ x: -1420, y: 370 }, mon125)).toEqual({ x: 400, y: 200 });
    expect(physicalToCssRect({ x: -1420, y: 370, w: 150, h: 50 }, mon125)).toEqual({
      x: 400,
      y: 200,
      w: 120,
      h: 40,
    });

    const mon200: OverlayMonitor = { originX: 0, originY: 0, width: 3840, height: 2160, scale: 2.0 };
    expect(physicalToCssRect({ x: 1000, y: 500, w: 200, h: 80 }, mon200)).toEqual({
      x: 500,
      y: 250,
      w: 100,
      h: 40,
    });
  });

  it("round-trips interactive CSS DOM rects to physical screen rects", () => {
    const mon: OverlayMonitor = { originX: 2560, originY: 0, width: 3840, height: 2160, scale: 1.5 };
    const phys = cssToPhysicalRect({ left: 100, top: 200, width: 320, height: 180 }, mon);
    expect(phys).toEqual({ x: 2710, y: 300, width: 480, height: 270 });
    expect(physicalToCssRect({ x: phys.x, y: phys.y, w: phys.width, h: phys.height }, mon)).toEqual({
      x: 100,
      y: 200,
      w: 320,
      h: 180,
    });
  });
});

describe("overlay placement", () => {
  const vp = { width: 1440, height: 900 };

  it("places a card beside its anchor without overlapping it", () => {
    const anchor = { x: 400, y: 300, w: 120, h: 40 };
    const pos = placeCard(anchor, vp, 300, 160);
    expect(rectsOverlap({ x: pos.x, y: pos.y, w: 300, h: 160 }, anchor)).toBe(false);
    expect(pos.x).toBeGreaterThanOrEqual(anchor.x + anchor.w);
  });

  it("flips card to the left when anchor is near the right edge", () => {
    const anchor = { x: 1300, y: 300, w: 100, h: 40 };
    const pos = placeCard(anchor, vp, 300, 160);
    expect(rectsOverlap({ x: pos.x, y: pos.y, w: 300, h: 160 }, anchor)).toBe(false);
    expect(pos.x + 300).toBeLessThanOrEqual(anchor.x);
  });

  it("places unanchored card in top-right safe area", () => {
    const pos = placeCard(undefined, vp, 300, 140);
    expect(pos.x).toBe(1440 - 300 - 24);
    expect(pos.y).toBeGreaterThanOrEqual(72);
  });

  it("places caption near target without overlapping the target or top island", () => {
    const topTarget = { x: 640, y: 40, w: 120, h: 32 };
    const below = placeCaption(undefined, topTarget, vp, 340, 74);
    expect(below.y).toBeGreaterThan(topTarget.y + topTarget.h);

    const bottomTarget = { x: 640, y: 840, w: 120, h: 36 };
    const above = placeCaption(undefined, bottomTarget, vp, 340, 74);
    expect(above.y + 74).toBeLessThan(bottomTarget.y);
  });

  it("stops arrow tip outside the target ring rather than covering its center", () => {
    const target = { x: 500, y: 400, w: 120, h: 40 };
    const curve = computeArrowGeometry({ x: 560, y: 420 }, undefined, target, vp);
    const insideTarget =
      curve.end.x > target.x &&
      curve.end.x < target.x + target.w &&
      curve.end.y > target.y &&
      curve.end.y < target.y + target.h;
    expect(insideTarget).toBe(false);
  });
});

describe("interactive hit regions", () => {
  it("checks physical hit rectangles with hysteresis margin", () => {
    const rects = [{ x: 100, y: 200, width: 300, height: 150 }];
    expect(isPointInHitRects(rects, 250, 250, 0)).toBe(true);
    expect(isPointInHitRects(rects, 96, 250, 0)).toBe(false);
    expect(isPointInHitRects(rects, 96, 250, 6)).toBe(true);
    expect(isPointInHitRects([], 250, 250, 6)).toBe(false);
  });

  it("routes the arrow around the caption pill", () => {
    const vp = { width: 1440, height: 900 };
    const to = { x: 300, y: 300 };
    // Caption right below and to the right of the target: the default
    // below-right diagonal would cross it.
    const caption = { x: 200, y: 330, w: 380, h: 110 };
    expect(segmentHitsRect({ x: 428, y: 392 }, to, caption)).toBe(true);
    const start = arrowStart(to, vp, [caption]);
    expect(segmentHitsRect(start, to, caption)).toBe(false);
    expect(Math.hypot(start.x - to.x, start.y - to.y)).toBeGreaterThan(60);
    // Nothing to avoid: the open diagonal.
    expect(arrowStart(to, vp)).toEqual({ x: 428, y: 392 });
  });
});
