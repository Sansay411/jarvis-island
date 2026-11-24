// Pure geometry helpers for the fullscreen area picker.

export interface LogicalRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PickerWindow {
  hwnd: number;
  title: string;
  process: string;
  rect: { x: number; y: number; width: number; height: number };
  localX: number;
  localY: number;
  localWidth: number;
  localHeight: number;
}

export interface PickerState {
  monitorBounds: { x: number; y: number; width: number; height: number };
  scale: number;
  logicalWidth: number;
  logicalHeight: number;
  windows: PickerWindow[];
}

const CLICK_THRESHOLD_PX = 6;

/** True when pointer down→up moved less than the drag threshold (single-click window pick). */
export function isClickGesture(x1: number, y1: number, x2: number, y2: number): boolean {
  return Math.hypot(x2 - x1, y2 - y1) < CLICK_THRESHOLD_PX;
}

/** Normalize drag endpoints in any direction and clamp to viewport bounds. */
export function normalizeLogicalRect(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  maxW: number,
  maxH: number,
): LogicalRect {
  const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
  const cx1 = clamp(x1, 0, maxW);
  const cy1 = clamp(y1, 0, maxH);
  const cx2 = clamp(x2, 0, maxW);
  const cy2 = clamp(y2, 0, maxH);
  const left = Math.min(cx1, cx2);
  const top = Math.min(cy1, cy2);
  return {
    x: left,
    y: top,
    width: Math.max(0, Math.max(cx1, cx2) - left),
    height: Math.max(0, Math.max(cy1, cy2) - top),
  };
}

/** Physical pixel dimensions of a logical rect at `scale`. */
export function physicalSizeOf(rect: LogicalRect, scale: number): { width: number; height: number } {
  const s = Number.isFinite(scale) && scale > 0 ? scale : 1;
  return {
    width: Math.max(0, Math.round(rect.width * s)),
    height: Math.max(0, Math.round(rect.height * s)),
  };
}

/** Topmost Z-order window containing `(x, y)` in monitor-local logical coords. */
export function hitTestWindow(x: number, y: number, windows: PickerWindow[]): PickerWindow | null {
  for (const w of windows) {
    if (x >= w.localX && y >= w.localY && x < w.localX + w.localWidth && y < w.localY + w.localHeight) {
      return w;
    }
  }
  return null;
}

/** Keep the live size pill inside the viewport, preferring just above top-left. */
export function sizeLabelPosition(
  rect: LogicalRect,
  viewportW: number,
  viewportH: number,
  pillW = 104,
  pillH = 26,
): { left: number; top: number } {
  const gap = 8;
  const maxLeft = Math.max(8, viewportW - pillW - 8);
  const left = Math.max(8, Math.min(rect.x, maxLeft));

  if (rect.y >= pillH + gap + 6) {
    return { left, top: rect.y - pillH - gap };
  }
  const below = rect.y + rect.height + gap;
  if (below + pillH + 6 <= viewportH) {
    return { left, top: below };
  }
  return { left: Math.min(rect.x + 8, maxLeft), top: Math.max(8, rect.y + 8) };
}
