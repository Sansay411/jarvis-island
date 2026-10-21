// JARVIS's eyes — the product's identity. Two amber pills that live at the
// top centre of the island in every state. They blink on rare timers (no
// continuous animation while idle), widen when you approach, narrow and
// scan while thinking, follow real audio levels while listening/speaking,
// curve into arcs on success and flatten on errors.

import { animate, motion, useMotionValue, useSpring, useTransform, type MotionValue } from "motion/react";
import { useEffect, useRef } from "react";

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
}

const EYE_W = 5.6;
const EYE_GAP = 13.5; // centre to centre
const BOX_W = 44;
const BOX_H = 30;

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

export function Eyes({ mood, level, lookX, lookY = 0, scale = 1 }: EyesProps) {
  const zero = useMotionValue(0);
  const lvl = level ?? zero;
  const gaze = lookX ?? zero;

  const baseH = useSpring(heightFor[mood], SPRING);
  const blink = useMotionValue(1);
  const drift = useMotionValue(0);
  const yOff = useSpring(lookY, SPRING);

  useEffect(() => {
    baseH.set(heightFor[mood]);
  }, [mood, baseH]);
  useEffect(() => {
    yOff.set(lookY);
  }, [lookY, yOff]);

  // Live audio drives height in voice moods; otherwise the spring rests.
  const audioDriven = mood === "listening" || mood === "speaking";
  const h = useTransform(() => {
    const b = baseH.get();
    const live = audioDriven ? lvl.get() * 13 : 0;
    return Math.max(1.6, (b + live) * blink.get());
  });
  const y = useTransform(() => BOX_H / 2 - h.get() / 2 + yOff.get());
  const gx = useTransform(() => gaze.get() * 2.4 + drift.get());
  const leftX = useTransform(() => BOX_W / 2 - EYE_GAP / 2 - EYE_W / 2 + gx.get());
  const rightX = useTransform(() => BOX_W / 2 + EYE_GAP / 2 - EYE_W / 2 + gx.get());
  const splitLeftX = useTransform(() => leftX.get() - 1.1);
  const splitRightX = useTransform(() => rightX.get() + 1.1);

  // Rare blinks, scheduled with timers so nothing runs between them.
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    const canBlink = mood === "idle" || mood === "attentive";
    if (!canBlink) return;
    const schedule = () => {
      const wait = 3500 + Math.random() * 5000;
      timer.current = window.setTimeout(() => {
        if (document.visibilityState === "visible") {
          const twice = Math.random() < 0.18;
          animate(blink, twice ? [1, 0.1, 1, 0.1, 1] : [1, 0.08, 1], {
            duration: twice ? 0.42 : 0.17,
            ease: "easeInOut",
          });
        }
        schedule();
      }, wait);
    };
    schedule();
    return () => window.clearTimeout(timer.current);
  }, [mood, blink]);

  // Thinking: eyes scan slowly left and right while work is in progress.
  useEffect(() => {
    if (mood !== "thinking") {
      animate(drift, 0, { duration: 0.25 });
      return;
    }
    const controls = animate(drift, [0, -2.2, 2.2, 0], {
      duration: 2.4,
      ease: "easeInOut",
      repeat: Infinity,
    });
    return () => controls.stop();
  }, [mood, drift]);

  const color = mood === "error" ? "var(--danger)" : "url(#eye-fill)";
  const showArcs = mood === "happy";
  const split = mood === "thinking";

  return (
    <svg
      width={BOX_W * scale}
      height={BOX_H * scale}
      viewBox={`0 0 ${BOX_W} ${BOX_H}`}
      style={{ overflow: "visible", display: "block" }}
      aria-hidden="true"
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
          <motion.rect x={splitLeftX} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill="#62d6ff" />
          <motion.rect x={splitRightX} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill="#ff6b8b" />
        </g>
      )}

      <motion.g
        filter={mood === "sleep" ? undefined : "url(#eye-glow)"}
        animate={{ opacity: showArcs ? 0 : mood === "sleep" ? 0.45 : 1 }}
        transition={{ duration: 0.18 }}
      >
        <motion.rect x={leftX} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill={color} />
        <motion.rect x={rightX} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill={color} />
      </motion.g>

      <motion.g
        filter="url(#eye-glow)"
        initial={false}
        animate={{ opacity: showArcs ? 1 : 0, y: showArcs ? 0 : 2 }}
        transition={{ duration: 0.22 }}
        stroke="url(#eye-fill)"
        strokeWidth={2.8}
        strokeLinecap="round"
        fill="none"
      >
        <path d={`M${BOX_W / 2 - EYE_GAP / 2 - 4} ${BOX_H / 2 + 2} Q${BOX_W / 2 - EYE_GAP / 2} ${BOX_H / 2 - 4.5} ${BOX_W / 2 - EYE_GAP / 2 + 4} ${BOX_H / 2 + 2}`} />
        <path d={`M${BOX_W / 2 + EYE_GAP / 2 - 4} ${BOX_H / 2 + 2} Q${BOX_W / 2 + EYE_GAP / 2} ${BOX_H / 2 - 4.5} ${BOX_W / 2 + EYE_GAP / 2 + 4} ${BOX_H / 2 + 2}`} />
      </motion.g>
    </svg>
  );
}
