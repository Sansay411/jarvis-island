// Shared timings and small pure helpers for the opened island's decks:
// the directional deck transition, the streak wave, count-up numbers and
// file-name truncation for shelf tiles. Everything here runs once per event,
// never as an idle loop.

import type { Transition, Variants } from "motion/react";
import type { PeekDeck } from "./geometry";

const DECK_ORDER: PeekDeck[] = ["nook", "focus", "tray", "activity", "sound"];

/** +1 when the new deck sits to the right of the old tab, -1 to the left. */
export function deckDirection(from: PeekDeck, to: PeekDeck): 1 | -1 | 0 {
  const a = DECK_ORDER.indexOf(from);
  const b = DECK_ORDER.indexOf(to);
  if (a === b) return 0;
  return b > a ? 1 : -1;
}

export const DECK_SPRING: Transition = { type: "spring", stiffness: 420, damping: 38 };

/** The new deck slides in from the side of the tab that was pressed and the
 *  old one leaves the other way, both through a light blur. */
export const deckVariants: Variants = {
  enter: (dir: number) => ({ x: 24 * dir, opacity: 0, filter: "blur(6px)" }),
  center: { x: 0, opacity: 1, filter: "blur(0px)" },
  exit: (dir: number) => ({ x: -24 * dir, opacity: 0, filter: "blur(6px)" }),
};

/** Reduced-motion variant: a plain cross-fade. */
export const deckFadeVariants: Variants = {
  enter: { opacity: 0 },
  center: { opacity: 1 },
  exit: { opacity: 0 },
};

/** `Bartender Materials` → `Barten…aterials`; the extension always survives. */
export function middleTruncate(name: string, max: number): string {
  const chars = Array.from(name);
  if (chars.length <= max) return name;
  if (max <= 1) return "…";
  const dot = name.lastIndexOf(".");
  const extLen = dot > 0 && name.length - dot <= 5 ? Array.from(name.slice(dot)).length : 0;
  const avail = max - 2;
  let head = Math.max(0, Math.floor(avail / 2) - 1);
  let tail = avail - head;
  if (extLen > 0 && tail < extLen + 2) {
    tail = Math.min(avail, extLen + 2);
    head = Math.max(0, avail - tail);
  }
  return `${chars.slice(0, head).join("")}…${tail > 0 ? chars.slice(-tail).join("") : ""}`;
}

/** Seconds of delay for a streak column: a left-to-right wave, ≤ 0.65 s. */
export function waveDelay(weekIndex: number, _totalWeeks: number): number {
  return Math.min(weekIndex * 0.012, 0.65);
}

/** Count-up value at progress `t` ∈ [0, 1], ease-out cubic, whole numbers. */
export function countUpValue(target: number, t: number): number {
  if (t >= 1) return target;
  if (t <= 0 || target === 0) return 0;
  const e = 1 - Math.pow(1 - t, 3);
  return Math.round(target * e);
}

export const FOCUS_PRESETS = [5, 15, 25, 50] as const;
