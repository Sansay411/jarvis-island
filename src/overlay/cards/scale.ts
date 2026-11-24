// Round axis ticks for card charts: 0 / 25 / 50 rather than 10,38 / 27,64.

export interface NiceScale {
  min: number;
  max: number;
  ticks: number[];
}

/** 1, 2, 2.5 or 5 times a power of ten, at least `raw`. */
function niceStep(raw: number): number {
  const pow = 10 ** Math.floor(Math.log10(raw));
  const f = raw / pow;
  const m = f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10;
  return m * pow;
}

/**
 * A range covering `lo..hi` on round steps with about `count` intervals.
 * Positive data starts at zero when zero is near (within one range).
 */
export function niceScale(lo: number, hi: number, count = 3): NiceScale {
  let a = Math.min(lo, hi);
  let b = Math.max(lo, hi);
  if (!Number.isFinite(a) || !Number.isFinite(b)) return { min: 0, max: 1, ticks: [0, 1] };
  if (a >= 0 && a <= b - a) a = 0;
  if (a === b) {
    const pad = Math.abs(a) * 0.1 || 1;
    a -= pad;
    b += pad;
    if (lo >= 0 && a < 0) a = 0;
  }
  const step = niceStep((b - a) / Math.max(1, count));
  const min = Math.floor(a / step + 1e-9) * step;
  const max = Math.ceil(b / step - 1e-9) * step;
  const ticks: number[] = [];
  for (let v = min; v <= max + step / 2 && ticks.length < 12; v += step) ticks.push(Number(v.toPrecision(12)));
  return { min, max, ticks };
}
