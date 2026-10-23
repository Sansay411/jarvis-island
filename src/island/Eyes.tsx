// JARVIS's eyes — the product's identity. Two amber pills that live at the
// top centre of the island in every state. They blink on rare timers, glance
// around now and then while idle, widen when you approach, narrow and scan
// while thinking, follow real audio levels while listening/speaking, curve
// into arcs on success and flatten on errors. On top of the moods come
// one-shot expressions (a wink, hearts, a laugh, surprise, a head shake)
// and a few habits: nodding to music, sleepy eyes at night, watching a
// timer's last seconds, jumping when it rings.

import { animate, motion, useMotionValue, useReducedMotion, useSpring, useTransform, type MotionValue } from "motion/react";
import { useEffect, useRef } from "react";
import type { Emote } from "../store/emote";

export type EyeMood = "idle" | "attentive" | "thinking" | "listening" | "speaking" | "happy" | "error" | "sleep";

interface EyesProps {
  mood: EyeMood;
  /** 0..1 live audio level (listening/speaking). */
  level?: MotionValue<number>;
  /** -1..1 horizontal gaze. */
  lookX?: MotionValue<number>;
  /** Vertical gaze offset in px (e.g. looking down at a drop zone). */
  lookY?: number;
  scale?: number;
  /** A one-shot expression; `emoteSeq` replays the same one. */
  emote?: Emote | null;
  emoteSeq?: number;
  /** Music is playing: nod along. */
  bob?: boolean;
  /** Late night: heavy lids, slow blinks. */
  sleepy?: boolean;
  /** A timer's last seconds: look at it (to the right), blink often. */
  watch?: boolean;
  /** A timer or reminder is ringing: jump. */
  alarm?: boolean;
  /** Double click on the eyes. */
  onBoop?: () => void;
}

const EYE_W = 5.6;
const EYE_GAP = 13.5; // centre to centre
const BOX_W = 44;
const BOX_H = 30;
const LEFT_CX = BOX_W / 2 - EYE_GAP / 2;
const RIGHT_CX = BOX_W / 2 + EYE_GAP / 2;

const heightFor: Record<EyeMood, number> = {
  idle: 12,
  attentive: 13.5,
  thinking: 7,
  listening: 9,
  speaking: 9,
  happy: 12,
  error: 3,
  sleep: 2.2,
};

const SPRING = { stiffness: 420, damping: 30, mass: 0.6 };

function arc(cx: number): string {
  return `M${cx - 4} ${BOX_H / 2 + 2} Q${cx} ${BOX_H / 2 - 4.5} ${cx + 4} ${BOX_H / 2 + 2}`;
}

function heart(cx: number): string {
  const cy = BOX_H / 2;
  return `M${cx} ${cy + 3.6} C${cx - 5.6} ${cy - 0.4} ${cx - 3.8} ${cy - 5.6} ${cx} ${cy - 2.4} C${cx + 3.8} ${cy - 5.6} ${cx + 5.6} ${cy - 0.4} ${cx} ${cy + 3.6} Z`;
}

export function Eyes({ mood, level, lookX, lookY = 0, scale = 1, emote = null, emoteSeq = 0, bob = false, sleepy = false, watch = false, alarm = false, onBoop }: EyesProps) {
  const reduced = useReducedMotion();
  const zero = useMotionValue(0);
  const lvl = level ?? zero;
  const gaze = lookX ?? zero;

  const resting = mood === "idle" && sleepy ? 6.5 : heightFor[mood];
  const baseH = useSpring(resting, SPRING);
  const blinkL = useMotionValue(1);
  const blinkR = useMotionValue(1);
  const widen = useMotionValue(1);
  const drift = useMotionValue(0);
  const glanceY = useMotionValue(0);
  const hop = useMotionValue(0);
  const yOff = useSpring(lookY, SPRING);

  useEffect(() => {
    baseH.set(resting);
  }, [resting, baseH]);
  useEffect(() => {
    yOff.set(lookY);
  }, [lookY, yOff]);

  // Live audio drives height in voice moods; otherwise the spring rests.
  const audioDriven = mood === "listening" || mood === "speaking";
  const open = () => (baseH.get() + (audioDriven ? lvl.get() * 13 : 0)) * widen.get();
  const hL = useTransform(() => Math.max(1.6, open() * blinkL.get()));
  const hR = useTransform(() => Math.max(1.6, open() * blinkR.get()));
  const lift = useTransform(() => yOff.get() + glanceY.get() + hop.get());
  const yL = useTransform(() => BOX_H / 2 - hL.get() / 2 + lift.get());
  const yR = useTransform(() => BOX_H / 2 - hR.get() / 2 + lift.get());
  const gx = useTransform(() => gaze.get() * 2.4 + drift.get());
  const leftX = useTransform(() => LEFT_CX - EYE_W / 2 + gx.get());
  const rightX = useTransform(() => RIGHT_CX - EYE_W / 2 + gx.get());
  const splitLeftX = useTransform(() => leftX.get() - 1.1);
  const splitRightX = useTransform(() => rightX.get() + 1.1);

  const blinkBoth = (twice = false, slow = false) => {
    const keys = twice ? [1, 0.1, 1, 0.1, 1] : [1, 0.08, 1];
    const duration = (twice ? 0.42 : 0.17) * (slow ? 2.2 : 1);
    animate(blinkL, keys, { duration, ease: "easeInOut" });
    animate(blinkR, keys, { duration, ease: "easeInOut" });
  };

  // Rare blinks, scheduled with timers so nothing runs between them; often
  // while watching a timer, slow and heavy at night.
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    const canBlink = mood === "idle" || mood === "attentive";
    if (!canBlink) return;
    const schedule = () => {
      const wait = watch ? 900 + Math.random() * 500 : 3500 + Math.random() * 5000;
      timer.current = window.setTimeout(() => {
        if (document.visibilityState === "visible") blinkBoth(!watch && Math.random() < 0.18, sleepy);
        schedule();
      }, wait);
    };
    schedule();
    return () => window.clearTimeout(timer.current);
  }, [mood, watch, sleepy]);

  // Idle: now and then a glance aside (and a little up or down), held a
  // moment, then back — the island looks alive without moving constantly.
  useEffect(() => {
    if (mood !== "idle" || reduced || watch || bob) return;
    let t: number | undefined;
    const schedule = () => {
      t = window.setTimeout(() => {
        if (document.visibilityState === "visible") {
          const side = (Math.random() < 0.5 ? -1 : 1) * (1.4 + Math.random() * 1.2);
          const up = (Math.random() - 0.5) * 2;
          animate(drift, [0, side, side, 0], { duration: 1.6, times: [0, 0.15, 0.8, 1], ease: "easeInOut" });
          animate(glanceY, [0, up, up, 0], { duration: 1.6, times: [0, 0.15, 0.8, 1], ease: "easeInOut" });
        }
        schedule();
      }, 7000 + Math.random() * 9000);
    };
    schedule();
    return () => window.clearTimeout(t);
  }, [mood, reduced, watch, bob]);

  // Thinking scans left and right; a timer's last seconds hold the gaze on
  // it (the countdown sits on the right).
  useEffect(() => {
    if (mood === "thinking") {
      const controls = animate(drift, [0, -2.2, 2.2, 0], { duration: 2.4, ease: "easeInOut", repeat: Infinity });
      return () => controls.stop();
    }
    animate(drift, watch ? 2.4 : 0, { duration: 0.25 });
  }, [mood, watch, drift]);

  // Music: a small nod on every beat-ish half second. A ringing alarm: hops.
  useEffect(() => {
    if (reduced || !(alarm || (bob && (mood === "idle" || mood === "attentive")))) {
      animate(hop, 0, { duration: 0.2 });
      return;
    }
    const controls = alarm
      ? animate(hop, [0, -2.4, 0], { duration: 0.34, ease: "easeOut", repeat: Infinity, repeatDelay: 0.08 })
      : animate(hop, [0, 1.1, 0], { duration: 0.56, ease: "easeInOut", repeat: Infinity });
    return () => controls.stop();
  }, [bob, alarm, mood, reduced, hop]);

  // An error shakes the head «no».
  useEffect(() => {
    if (mood === "error" && !reduced) animate(drift, [0, -3, 3, -2.4, 2.4, -1, 0], { duration: 0.55 });
  }, [mood, reduced, drift]);

  // One-shot expressions.
  useEffect(() => {
    if (!emote) return;
    switch (emote) {
      case "wink":
        animate(blinkR, [1, 0.08, 0.08, 1], { duration: 0.6, times: [0, 0.2, 0.75, 1] });
        break;
      case "surprised":
        animate(widen, [1, 1.4, 1.4, 1], { duration: 0.8, times: [0, 0.15, 0.7, 1], ease: "easeOut" });
        break;
      case "shake":
        if (!reduced) animate(drift, [0, -3, 3, -2.4, 2.4, -1, 0], { duration: 0.55 });
        break;
      case "laugh":
      case "boop":
        if (!reduced) animate(hop, [0, -1.6, 0, -1.6, 0, -0.8, 0], { duration: emote === "laugh" ? 0.9 : 0.6 });
        break;
      case "heart":
        break;
    }
  }, [emote, emoteSeq]);

  const color = mood === "error" ? "var(--danger)" : "url(#eye-fill)";
  const showArcs = mood === "happy" || emote === "laugh" || emote === "boop";
  const showHearts = emote === "heart";
  const split = mood === "thinking";
  const pillsHidden = showArcs || showHearts;

  return (
    <svg
      width={BOX_W * scale}
      height={BOX_H * scale}
      viewBox={`0 0 ${BOX_W} ${BOX_H}`}
      style={{ overflow: "visible", display: "block", pointerEvents: onBoop ? "auto" : undefined }}
      aria-hidden="true"
      onDoubleClick={onBoop}
    >
      <defs>
        <linearGradient id="eye-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="var(--amber-hi)" />
          <stop offset="0.55" stopColor="var(--amber)" />
          <stop offset="1" stopColor="var(--amber-lo)" />
        </linearGradient>
        <filter id="eye-glow" x="-200%" y="-100%" width="500%" height="300%">
          <feGaussianBlur stdDeviation="2.2" result="b" />
          <feMerge>
            <feMergeNode in="b" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {/* Restrained chromatic split: only while reasoning. */}
      {split && (
        <g opacity={0.5} style={{ mixBlendMode: "screen" }}>
          <motion.rect x={splitLeftX} y={yL} width={EYE_W} height={hL} rx={EYE_W / 2} fill="#62d6ff" />
          <motion.rect x={splitRightX} y={yR} width={EYE_W} height={hR} rx={EYE_W / 2} fill="#ff6b8b" />
        </g>
      )}

      <motion.g
        filter={mood === "sleep" ? undefined : "url(#eye-glow)"}
        animate={{ opacity: pillsHidden ? 0 : mood === "sleep" ? 0.45 : 1 }}
        transition={{ duration: 0.18 }}
      >
        <motion.rect x={leftX} y={yL} width={EYE_W} height={hL} rx={EYE_W / 2} fill={color} />
        <motion.rect x={rightX} y={yR} width={EYE_W} height={hR} rx={EYE_W / 2} fill={color} />
      </motion.g>

      <motion.g
        filter="url(#eye-glow)"
        initial={false}
        animate={{ opacity: showArcs ? 1 : 0, y: showArcs ? 0 : 2 }}
        transition={{ duration: 0.22 }}
        style={{ y: hop }}
        stroke="url(#eye-fill)"
        strokeWidth={2.8}
        strokeLinecap="round"
        fill="none"
      >
        <path d={arc(LEFT_CX)} />
        <path d={arc(RIGHT_CX)} />
      </motion.g>

      <motion.g
        filter="url(#eye-glow)"
        initial={false}
        animate={showHearts ? { opacity: 1, scale: [0.4, 1.25, 1, 1.12, 1] } : { opacity: 0, scale: 0.6 }}
        transition={showHearts ? { duration: 0.9, times: [0, 0.3, 0.5, 0.75, 1] } : { duration: 0.2 }}
        style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        fill="url(#eye-fill)"
      >
        <path d={heart(LEFT_CX)} />
        <path d={heart(RIGHT_CX)} />
      </motion.g>
    </svg>
  );
}
