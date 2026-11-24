// Item lifetimes and merging for the overlay, mirroring `overlay.rs`
// (`item_lifetime`, the `replace` / same-`id` rules of `draw`).

import type { OverlayItem } from "./types";

/** Lifetime of rings, arrows and captions without `seconds`. */
export const DEFAULT_ITEM_SECONDS = 8;

export interface ActiveItem {
  key: string;
  item: OverlayItem;
  /** `Date.now()` time to drop the item, `null` = until replaced or closed. */
  expiresAt: number | null;
}

/** Milliseconds an item stays on screen; `null` for cards and waiting teach steps. */
export function itemLifetimeMs(item: OverlayItem): number | null {
  if (item.kind === "card" || item.persistent) return null;
  const secs = typeof item.seconds === "number" ? item.seconds : DEFAULT_ITEM_SECONDS;
  return Number.isFinite(secs) && secs > 0 ? Math.min(secs, 3600) * 1000 : null;
}

let seq = 1;

export function wrapItem(item: OverlayItem, now: number): ActiveItem {
  const key = item.id ? item.id : `ov-${seq++}`;
  const life = itemLifetimeMs(item);
  return { key, item, expiresAt: life === null ? null : now + life };
}

/** Apply one `overlay:draw`: replace everything, or update same-key items and add the rest. */
export function mergeItems(prev: ActiveItem[], incoming: ActiveItem[], replace: boolean): ActiveItem[] {
  if (replace) return incoming;
  const next = [...prev];
  for (const item of incoming) {
    const idx = next.findIndex((x) => x.key === item.key);
    if (idx >= 0) next[idx] = item;
    else next.push(item);
  }
  return next;
}

/** Items still alive at `now`. */
export function liveItems(items: ActiveItem[], now: number): ActiveItem[] {
  return items.filter((a) => a.expiresAt === null || a.expiresAt > now);
}
