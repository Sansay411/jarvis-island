// Island geometry. The island hangs from the top edge of the display like a
// hardware notch: flat top at y = 0, smooth 14px concave shoulders that fuse
// into the bezel, and calm bottom corners whose radius scales with height.

export const CANVAS_W = 760;
export const CANVAS_H = 640;
/** Radius of the concave shoulders at the top corners. */
export const SHOULDER = 14;
/** Height of the persistent top row (the "brow") that holds the centered eyes. */
export const BROW_H = 36;
/** Height of the resting island over maximised windows. */
export const LIP_H = 10;

/** Shoulders shrink with very short shapes so the lip stays a clean tab. */
export function shoulderFor(h: number): number {
  return Math.min(SHOULDER, Math.max(4, h * 0.55));
}

export type IslandMode = "compact" | "peek" | "workspace" | "voice" | "drop" | "files" | "notice" | "clips";
export type PeekDeck = "nook" | "focus" | "tray" | "activity";

export interface Dims {
  w: number;
  h: number;
}

export interface DimsContext {
  hasMedia: boolean;
  hasActivity: boolean;
  hasFocus?: boolean;
  ttsSpeaking?: boolean;
  missionActive?: boolean;
  peekDeck?: PeekDeck;
  focusBanner?: boolean;
  /** Running background jobs shown as rows in the peek view (max 2). */
  jobRows: number;
  /** Timer and stopwatch rows in the peek view (max 3). */
  clockRows?: number;
  /** A maximised window's title bar is under the island: rest as a thin lip. */
  lip: boolean;
  /** A confirmation is waiting (shown inline in the voice view). */
  confirm: boolean;
  /** A timer or stopwatch pill in the voice view. */
  voiceClock?: boolean;
  /** Measured height of the file card. */
  filesH?: number;
  /** Measured height of the clipboard panel. */
  clipsH?: number;
}

export function dimsFor(mode: IslandMode, ctx: DimsContext): Dims {
  switch (mode) {
    case "compact": {
      if (ctx.lip) return { w: 132, h: LIP_H };
      const dual = ctx.hasMedia && Boolean(ctx.hasFocus || ctx.hasActivity || ctx.ttsSpeaking);
      if (dual) return { w: 262, h: BROW_H };
      if (ctx.hasMedia || ctx.hasFocus || ctx.hasActivity || ctx.ttsSpeaking) return { w: 228, h: BROW_H };
      return { w: 174, h: BROW_H };
    }
    case "peek": {
      const stripRows = ctx.jobRows + (ctx.clockRows ?? 0) + (ctx.missionActive ? 1 : 0);
      const extraH = stripRows * 28;
      if (ctx.focusBanner) {
        return { w: 584, h: 146 + extraH };
      }
      switch (ctx.peekDeck ?? "nook") {
        case "focus":
          return { w: 624, h: 294 + extraH };
        case "tray":
          return { w: 604, h: 236 + extraH };
        case "activity":
          return { w: 556, h: 234 + extraH };
        case "nook":
        default:
          return { w: 616, h: 238 + extraH };
      }
    }
    case "voice":
      return { w: 408, h: 98 + (ctx.voiceClock ? 34 : 0) + (ctx.confirm ? 48 : 0) };
    case "drop":
      return { w: 468, h: 144 };
    case "notice":
      return { w: 428, h: 128 };
    case "files":
      return { w: 492, h: Math.min(450, Math.max(156, Math.round(ctx.filesH ?? 0))) };
    case "clips":
      return { w: 472, h: Math.min(440, Math.max(174, Math.round(ctx.clipsH ?? 0))) };
    case "workspace":
      return { w: 528, h: 492 };
  }
}

export function bottomRadius(h: number): number {
  // Compact is a crisp hardware capsule; taller decks settle on a calm 24px squircle.
  if (h <= BROW_H + 2) return Math.min(h / 2, 16);
  return Math.min(24, 16 + (h - BROW_H) * 0.08);
}

/**
 * SVG path of the notch, drawn in a box of (w + 2s) × h where `s` is the
 * concave shoulder radius. The body spans x ∈ [s, s + w], flush at y = 0.
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

/** Interactive rect sent to the native hit-tester (`spanW` widens hit-testing when bezel satellites are shown). */
export function shapeRect(d: Dims, spanW?: number) {
  const w = Math.min(CANVAS_W, Math.max(d.w, spanW ?? d.w));
  return { x: (CANVAS_W - w) / 2, y: 0, width: w, height: d.h };
}
