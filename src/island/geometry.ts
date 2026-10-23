// Island geometry. The island hangs from the top edge of the display like a
// notch: flat top, concave "shoulders" that melt into the screen edge, and
// rounded bottom corners whose radius grows with the island's height.

export const CANVAS_W = 640;
export const CANVAS_H = 600;
/** Radius of the concave shoulders at the top corners. */
export const SHOULDER = 10;
/** Height of the persistent top row (the "brow") that holds the eyes. */
export const BROW_H = 34;
/** Height of the resting island over maximised windows. */
export const LIP_H = 10;

/** Shoulders shrink with very short shapes so the lip stays a clean tab. */
export function shoulderFor(h: number): number {
  return Math.min(SHOULDER, Math.max(4, h * 0.6));
}

export type IslandMode = "compact" | "peek" | "workspace" | "voice" | "drop" | "files" | "notice";

export interface Dims {
  w: number;
  h: number;
}

export interface DimsContext {
  hasMedia: boolean;
  hasActivity: boolean;
  /** Running background jobs shown as rows in the peek view (max 2). */
  jobRows: number;
  /** Timer and stopwatch rows in the peek view (max 3). */
  clockRows?: number;
  /** A maximised window's title bar is under the island: rest as a thin lip. */
  lip: boolean;
  /** A confirmation is waiting (shown inline in the voice view). */
  confirm: boolean;
  /** Measured height of the file card. */
  filesH?: number;
}

export function dimsFor(mode: IslandMode, ctx: DimsContext): Dims {
  switch (mode) {
    case "compact":
      if (ctx.lip) return { w: 132, h: LIP_H };
      return { w: ctx.hasMedia || ctx.hasActivity ? 212 : 168, h: BROW_H };
    case "peek":
    {
      const rows = ctx.jobRows + (ctx.clockRows ?? 0);
      return ctx.hasMedia ? { w: 452, h: 136 + rows * 30 } : { w: 412, h: 104 + rows * 30 };
    }
    case "voice":
      return { w: 380, h: ctx.confirm ? 178 : 132 };
    case "drop":
      return { w: 440, h: 132 };
    case "notice":
      return { w: 420, h: 124 };
    case "files":
      return { w: 480, h: Math.min(440, Math.max(150, Math.round(ctx.filesH ?? 0))) };
    case "workspace":
      return { w: 528, h: 492 };
  }
}

export function bottomRadius(h: number): number {
  // Compact is a near-pill; taller states settle on a calm 26px corner.
  if (h <= BROW_H + 2) return Math.min(h / 2, 15);
  return Math.min(26, 15 + (h - BROW_H) * 0.12);
}

/**
 * SVG path of the notch, drawn in a box of (w + 2s) × h where `s` is the
 * shoulder radius. The body spans x ∈ [s, s + w].
 */
export function notchPath(w: number, h: number, r: number, s: number = SHOULDER): string {
  const L = s;
  const R = s + w;
  const rr = Math.max(0, Math.min(r, w / 2, h - s));
  return [
    `M0,0`,
    // left shoulder: concave quarter circle centred at (0, s)
    `A${s},${s} 0 0 1 ${L},${s}`,
    `L${L},${h - rr}`,
    `A${rr},${rr} 0 0 0 ${L + rr},${h}`,
    `L${R - rr},${h}`,
    `A${rr},${rr} 0 0 0 ${R},${h - rr}`,
    `L${R},${s}`,
    // right shoulder, mirrored
    `A${s},${s} 0 0 1 ${R + s},0`,
    `Z`,
  ].join(" ");
}

/** Interactive rect sent to the native hit-tester (body only, no shoulders). */
export function shapeRect(d: Dims) {
  return { x: (CANVAS_W - d.w) / 2, y: 0, width: d.w, height: d.h };
}
