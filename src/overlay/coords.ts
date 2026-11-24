// Pure coordinate conversion, layout placement and hit-region helpers for the
// transparent full-monitor overlay. Kept free of DOM/Tauri dependencies so
// every calculation is unit-tested across DPI scales and multi-monitor layouts.

import type { CssRect, OverlayMonitor, Point2D, RawBox, ScreenRect } from "./types";

export interface ViewportSize {
  width: number;
  height: number;
}

export interface ArrowCurve {
  start: Point2D;
  control: Point2D;
  end: Point2D;
  angleDeg: number;
}

const DEFAULT_SCALE = 1;
const ISLAND_SAFE_TOP = 72;

function safeScale(scale: number): number {
  return Number.isFinite(scale) && scale > 0 ? scale : DEFAULT_SCALE;
}

/** Convert a physical screen pixel coordinate into overlay CSS pixels. */
export function physicalToCssPoint(pt: Point2D, mon: OverlayMonitor): Point2D {
  const s = safeScale(mon.scale);
  return {
    x: (pt.x - mon.originX) / s,
    y: (pt.y - mon.originY) / s,
  };
}

/** Convert a physical screen box `{x,y,w,h}` into overlay CSS pixels. */
export function physicalToCssRect(box: RawBox, mon: OverlayMonitor): CssRect {
  const s = safeScale(mon.scale);
  return {
    x: (box.x - mon.originX) / s,
    y: (box.y - mon.originY) / s,
    w: box.w / s,
    h: box.h / s,
  };
}

/** Convert an overlay DOM CSS rect (`getBoundingClientRect`) to physical screen pixels. */
export function cssToPhysicalRect(
  rect: { left: number; top: number; width: number; height: number },
  mon: OverlayMonitor,
): ScreenRect {
  const s = safeScale(mon.scale);
  return {
    x: mon.originX + Math.round(rect.left * s),
    y: mon.originY + Math.round(rect.top * s),
    width: Math.max(0, Math.round(rect.width * s)),
    height: Math.max(0, Math.round(rect.height * s)),
  };
}

/** True when two CSS rectangles intersect (with optional spacing gap). */
export function rectsOverlap(a: CssRect, b: CssRect, gap = 0): boolean {
  return (
    a.x < b.x + b.w + gap &&
    a.x + a.w + gap > b.x &&
    a.y < b.y + b.h + gap &&
    a.y + a.h + gap > b.y
  );
}

/** Clamp a floating box inside viewport bounds with padding. */
export function clampToViewport(
  x: number,
  y: number,
  w: number,
  h: number,
  vp: ViewportSize,
  pad = 16,
  topPad = pad,
): Point2D {
  const maxX = Math.max(pad, vp.width - w - pad);
  const maxY = Math.max(topPad, vp.height - h - pad);
  return {
    x: Math.min(Math.max(x, pad), maxX),
    y: Math.min(Math.max(y, topPad), maxY),
  };
}

/**
 * Place a card next to `anchor` without covering it (preferring right, then
 * left, below, above), or top-right below the island when there is no anchor.
 */
export function placeCard(
  anchor: CssRect | undefined,
  vp: ViewportSize,
  cardW: number,
  cardH: number,
  gap = 16,
): Point2D {
  const pad = 16;
  if (!anchor || anchor.w <= 0 || anchor.h <= 0) {
    return clampToViewport(vp.width - cardW - 24, ISLAND_SAFE_TOP, cardW, cardH, vp, pad, ISLAND_SAFE_TOP);
  }

  const candidates: Point2D[] = [
    // Right of anchor
    { x: anchor.x + anchor.w + gap, y: anchor.y + (anchor.h - cardH) / 2 },
    // Left of anchor
    { x: anchor.x - cardW - gap, y: anchor.y + (anchor.h - cardH) / 2 },
    // Below anchor
    { x: anchor.x + (anchor.w - cardW) / 2, y: anchor.y + anchor.h + gap },
    // Above anchor
    { x: anchor.x + (anchor.w - cardW) / 2, y: anchor.y - cardH - gap },
  ];

  for (const c of candidates) {
    const clamped = clampToViewport(c.x, c.y, cardW, cardH, vp, pad, pad);
    const box: CssRect = { x: clamped.x, y: clamped.y, w: cardW, h: cardH };
    if (!rectsOverlap(box, anchor, 6)) {
      return clamped;
    }
  }

  // Fallback: corner furthest from anchor center.
  const anchorCx = anchor.x + anchor.w / 2;
  const x = anchorCx > vp.width / 2 ? pad : vp.width - cardW - pad;
  return clampToViewport(x, ISLAND_SAFE_TOP, cardW, cardH, vp, pad, ISLAND_SAFE_TOP);
}

/**
 * Place a caption pill cleanly near `targetRect` (without covering it or the
 * top-center island), or at an explicit `(x, y)` point when provided.
 */
export function placeCaption(
  explicit: Point2D | undefined,
  targetRect: CssRect | undefined,
  vp: ViewportSize,
  pillW: number,
  pillH: number,
): Point2D {
  const pad = 18;
  if (explicit) {
    return clampToViewport(explicit.x - pillW / 2, explicit.y, pillW, pillH, vp, pad, pad);
  }
  if (!targetRect || targetRect.w <= 0 || targetRect.h <= 0) {
    // Centered cleanly below the Dynamic Island zone.
    return clampToViewport((vp.width - pillW) / 2, ISLAND_SAFE_TOP + 12, pillW, pillH, vp, pad, ISLAND_SAFE_TOP);
  }

  const gap = 22;
  const cx = targetRect.x + targetRect.w / 2 - pillW / 2;
  const belowY = targetRect.y + targetRect.h + gap;
  const aboveY = targetRect.y - pillH - gap;

  // Prefer below when it fits without clipping; else try above (keeping clear of the island).
  if (belowY + pillH + pad <= vp.height) {
    return clampToViewport(cx, belowY, pillW, pillH, vp, pad, ISLAND_SAFE_TOP);
  }
  if (aboveY >= ISLAND_SAFE_TOP) {
    return clampToViewport(cx, aboveY, pillW, pillH, vp, pad, ISLAND_SAFE_TOP);
  }
  // Otherwise place beside the target on the side with more room.
  const rightSpace = vp.width - (targetRect.x + targetRect.w);
  const sideX = rightSpace >= targetRect.x ? targetRect.x + targetRect.w + gap : targetRect.x - pillW - gap;
  return clampToViewport(sideX, targetRect.y + (targetRect.h - pillH) / 2, pillW, pillH, vp, pad, ISLAND_SAFE_TOP);
}

/**
 * Compute a smooth quadratic Bezier arrow pointing to `to` (stopping neatly at
 * the outer margin of `targetRect` when provided so the tip never covers the control).
 */
export function computeArrowGeometry(
  to: Point2D,
  from: Point2D | undefined,
  targetRect: CssRect | undefined,
  vp: ViewportSize,
): ArrowCurve {
  let start = from;
  if (!start) {
    // Approach from whichever horizontal/vertical quadrant has more open space.
    const fromRight = to.x < vp.width * 0.65;
    const fromBelow = to.y < vp.height * 0.62;
    const dx = fromRight ? 108 : -108;
    const dy = fromBelow ? 78 : -78;
    start = clampToViewport(to.x + dx, to.y + dy, 8, 8, vp, 24, ISLAND_SAFE_TOP);
  }

  // Pull the arrow tip back to the outer edge of the target ring (+10px breathing space).
  let end = { ...to };
  if (targetRect && targetRect.w > 0 && targetRect.h > 0) {
    const cx = targetRect.x + targetRect.w / 2;
    const cy = targetRect.y + targetRect.h / 2;
    const vx = start.x - cx;
    const vy = start.y - cy;
    const len = Math.hypot(vx, vy);
    if (len > 1) {
      const rx = targetRect.w / 2 + 12;
      const ry = targetRect.h / 2 + 12;
      const tx = Math.abs(vx) > 1e-4 ? rx / Math.abs(vx) : Infinity;
      const ty = Math.abs(vy) > 1e-4 ? ry / Math.abs(vy) : Infinity;
      const t = Math.min(tx, ty);
      if (Number.isFinite(t) && t < 0.85) {
        end = { x: cx + vx * t, y: cy + vy * t };
      }
    }
  }

  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const dist = Math.hypot(dx, dy) || 1;
  // Subtle perpendicular bend for an organic curved pointer.
  const bend = Math.min(34, dist * 0.22) * (dx >= 0 ? -1 : 1);
  const nx = -dy / dist;
  const ny = dx / dist;
  const control: Point2D = {
    x: (start.x + end.x) / 2 + nx * bend,
    y: (start.y + end.y) / 2 + ny * bend,
  };

  // Tangent at t = 1 of quadratic Bezier is (end - control).
  const angleDeg = (Math.atan2(end.y - control.y, end.x - control.x) * 180) / Math.PI;
  return { start, control, end, angleDeg };
}

/** Pure hit-test matching the backend's interactive rect rule. */
export function isPointInHitRects(rects: readonly ScreenRect[], px: number, py: number, margin = 0): boolean {
  return rects.some(
    (r) =>
      r.width > 0 &&
      r.height > 0 &&
      px >= r.x - margin &&
      px < r.x + r.width + margin &&
      py >= r.y - margin &&
      py < r.y + r.height + margin,
  );
}
