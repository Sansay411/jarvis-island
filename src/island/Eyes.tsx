// JARVIS's eyes — the product's identity. Two amber pills that live at the
// top centre of the island in every state. They blink on rare timers, glance
// around now and then while idle, widen when you approach, narrow and scan
// while thinking, follow real audio levels while listening/speaking, curve
// into arcs on success and flatten on errors. On top of the moods come
// one-shot expressions (a wink, hearts, a laugh, surprise, a head shake)
// and a few habits: nodding to music, sleepy eyes at night, watching a
// timer's last seconds, jumping when it rings.
//
// Each eye is a small lens of light, drawn in layers: a two-step halo, the
// amber body with side shading (so it reads round), a hot inner core, a thin
// light rim, a glint that drifts against the gaze, and a faint reflection on
// the obsidian below. A very slow shimmer keeps it alive.

import { animate, motion, useMotionValue, useReducedMotion, useSpring, useTransform, type MotionValue } from "motion/react";
import { useEffect, useId, useRef } from "react";
import type { Activity, Emote } from "../store/emote";

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
  /** What JARVIS is busy with: looking up, reading, typing, scanning. */
  activity?: Activity | null;
  /** Double click on the eyes. */
  onBoop?: () => void;
}

const EYE_W = 6.4;
const EYE_GAP = 14.5; // centre to centre
const BOX_W = 44;
const BOX_H = 30;
const LEFT_CX = BOX_W / 2 - EYE_GAP / 2;
const RIGHT_CX = BOX_W / 2 + EYE_GAP / 2;

const heightFor: Record<EyeMood, number> = {
  idle: 13,
  attentive: 14.6,
  thinking: 7.5,
  listening: 9.6,
  speaking: 9.6,
  happy: 13,
  error: 3.2,
  sleep: 2.4,
};

interface Ids {
  fill: string;
  shade: string;
  core: string;
  glow: string;
  refl: string;
}

/** The part of an eye left open by its lid: everything below a line through
 *  the eye's centre column, `drop` (0 open … 1 half shut) down from its top,
 *  tilted by `tilt` degrees. Applied as a clip, so the glow follows the cut. */
function lidClip(cx: number, top: number, h: number, drop: number, tilt: number): string {
  const X = 14;
  const line = top - 1.5 + drop * h * 0.55;
  const t = Math.tan((tilt * Math.PI) / 180);
  const bottom = top + h + 40;
  return `M${cx - X} ${line - t * X} L${cx + X} ${line + t * X} L${cx + X} ${bottom} L${cx - X} ${bottom} Z`;
}

/** One eye, layer by layer (see the header); `clip` is its lid. */
function Pill({ x, y, h, gx, ids, error, clip }: { x: MotionValue<number>; y: MotionValue<number>; h: MotionValue<number>; gx: MotionValue<number>; ids: Ids; error: boolean; clip: string }) {
  const coreW = EYE_W * 0.4;
  const coreX = useTransform(() => x.get() + (EYE_W - coreW) / 2);
  const coreY = useTransform(() => y.get() + h.get() * 0.14 + 0.4);
  const coreH = useTransform(() => Math.max(0, h.get() * 0.72 - 0.8));
  // The glint sits up and to the left, and slides a little against the gaze.
  const glintX = useTransform(() => x.get() + EYE_W * 0.33 - gx.get() * 0.22);
  const glintY = useTransform(() => y.get() + Math.min(2.7, h.get() * 0.2) + 0.2);
  const glintR = useTransform(() => (h.get() > 4.5 ? Math.min(1.05, h.get() * 0.085) : 0));
  return (
    <>
      {/* The glow is built from the eye as the lid cuts it. */}
      <g filter={`url(#${ids.glow})`}>
        <g clipPath={`url(#${clip})`}>
          <motion.rect x={x} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill={error ? "var(--danger)" : `url(#${ids.fill})`} />
        </g>
      </g>
      <g clipPath={`url(#${clip})`}>
        <motion.rect x={x} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill={`url(#${ids.shade})`} />
        <motion.rect x={coreX} y={coreY} width={coreW} height={coreH} rx={coreW / 2} fill={error ? "#ffd2ca" : `url(#${ids.core})`} opacity={0.8} />
        <motion.rect x={x} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill="none" stroke={error ? "rgba(255,200,190,0.4)" : "rgba(255,240,212,0.38)"} strokeWidth={0.45} />
        <motion.circle cx={glintX} cy={glintY} r={glintR} fill="#fffaf0" opacity={0.92} />
      </g>
    </>
  );
}

/** The faint light an eye throws on the island surface below it. */
function Reflection({ x, y, h, ids }: { x: MotionValue<number>; y: MotionValue<number>; h: MotionValue<number>; ids: Ids }) {
  const cx = useTransform(() => x.get() + EYE_W / 2);
  const cy = useTransform(() => y.get() + h.get() + 3.4);
  return <motion.ellipse cx={cx} cy={cy} rx={3.6} ry={1.15} fill={`url(#${ids.refl})`} />;
}

const SPRING = { stiffness: 420, damping: 30, mass: 0.6 };

function arc(cx: number): string {
  return `M${cx - 4} ${BOX_H / 2 + 2} Q${cx} ${BOX_H / 2 - 4.5} ${cx + 4} ${BOX_H / 2 + 2}`;
}

/** A four-point sparkle. */
function star(cx: number): string {
  const cy = BOX_H / 2;
  const r = 5.2;
  const i = 1.3;
  return `M${cx} ${cy - r} Q${cx + i} ${cy - i} ${cx + r} ${cy} Q${cx + i} ${cy + i} ${cx} ${cy + r} Q${cx - i} ${cy + i} ${cx - r} ${cy} Q${cx - i} ${cy - i} ${cx} ${cy - r} Z`;
}

function heart(cx: number): string {
  const cy = BOX_H / 2;
  return `M${cx} ${cy + 3.6} C${cx - 5.6} ${cy - 0.4} ${cx - 3.8} ${cy - 5.6} ${cx} ${cy - 2.4} C${cx + 3.8} ${cy - 5.6} ${cx + 5.6} ${cy - 0.4} ${cx} ${cy + 3.6} Z`;
}

export function Eyes({ mood, level, lookX, lookY = 0, scale = 1, emote = null, emoteSeq = 0, bob = false, sleepy = false, watch = false, alarm = false, activity = null, onBoop }: EyesProps) {
  const reduced = useReducedMotion();
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const ids: Ids = { fill: `eye-fill-${uid}`, shade: `eye-shade-${uid}`, core: `eye-core-${uid}`, glow: `eye-glow-${uid}`, refl: `eye-refl-${uid}` };
  const clipL = `eye-lid-l-${uid}`;
  const clipR = `eye-lid-r-${uid}`;
  const zero = useMotionValue(0);
  const life = useMotionValue(1);
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
  // Per eye: height factor (squint, confusion) and eyelid drop and angle
  // (sad: outer corners down; determined: inner corners down).
  const sizeL = useMotionValue(1);
  const sizeR = useMotionValue(1);
  const lidL = useMotionValue(0);
  const lidR = useMotionValue(0);
  const tiltL = useMotionValue(0);
  const tiltR = useMotionValue(0);
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
  const hL = useTransform(() => Math.max(1.6, open() * blinkL.get() * sizeL.get()));
  const hR = useTransform(() => Math.max(1.6, open() * blinkR.get() * sizeR.get()));
  const lift = useTransform(() => yOff.get() + glanceY.get() + hop.get());
  const yL = useTransform(() => BOX_H / 2 - hL.get() / 2 + lift.get());
  const yR = useTransform(() => BOX_H / 2 - hR.get() / 2 + lift.get());
  const gx = useTransform(() => gaze.get() * 2.4 + drift.get());
  const leftX = useTransform(() => LEFT_CX - EYE_W / 2 + gx.get());
  const rightX = useTransform(() => RIGHT_CX - EYE_W / 2 + gx.get());
  const lidPathL = useTransform(() => lidClip(leftX.get() + EYE_W / 2, yL.get(), hL.get(), lidL.get(), tiltL.get()));
  const lidPathR = useTransform(() => lidClip(rightX.get() + EYE_W / 2, yR.get(), hR.get(), lidR.get(), tiltR.get()));
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

  // Busy with a tool: searching looks up and aside (recalling), reading
  // runs along lines below, typing flicks down at the keys, scanning the
  // screen sweeps wide-eyed.
  useEffect(() => {
    if (!activity || reduced) return;
    const runs: { stop: () => void }[] = [];
    switch (activity) {
      case "search":
        runs.push(animate(drift, 1.9, { duration: 0.3 }), animate(glanceY, -2.4, { duration: 0.3 }));
        break;
      case "read":
        runs.push(animate(glanceY, 1.8, { duration: 0.3 }), animate(drift, [-1.8, 1.8, -1.8], { duration: 1.3, ease: "linear", repeat: Infinity, repeatType: "loop", times: [0, 0.85, 1] }));
        break;
      case "type":
        runs.push(animate(glanceY, 2.2, { duration: 0.25 }), animate(drift, [0, -0.7, 0.5, -0.3, 0.8, 0], { duration: 0.9, repeat: Infinity }));
        break;
      case "scan":
        runs.push(animate(widen, 1.15, { duration: 0.3 }), animate(drift, [-2.4, 2.4], { duration: 0.9, ease: "easeInOut", repeat: Infinity, repeatType: "reverse" }));
        break;
    }
    return () => {
      runs.forEach((r) => r.stop());
      animate(drift, 0, { duration: 0.25 });
      animate(glanceY, 0, { duration: 0.25 });
      animate(widen, 1, { duration: 0.25 });
    };
  }, [activity, reduced]);

  // Thinking scans left and right; a timer's last seconds hold the gaze on
  // it (the countdown sits on the right).
  useEffect(() => {
    if (activity) return;
    if (mood === "thinking") {
      const controls = animate(drift, [0, -2.2, 2.2, 0], { duration: 2.4, ease: "easeInOut", repeat: Infinity });
      return () => controls.stop();
    }
    animate(drift, watch ? 2.4 : 0, { duration: 0.25 });
  }, [mood, watch, drift, activity]);

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

  // A slow, barely visible shimmer: the light is alive, not printed. Only
  // while JARVIS is busy: an endless animation repaints the blurred SVG at
  // the panel's refresh rate and keeps DWM compositing an always-on-top
  // transparent window — the idle island must cost nothing between blinks.
  useEffect(() => {
    if (reduced || mood === "sleep" || mood === "idle" || mood === "attentive") {
      life.set(1);
      return;
    }
    const controls = animate(life, [1, 0.88, 1, 0.94, 1], { duration: 5.5, ease: "easeInOut", repeat: Infinity });
    return () => controls.stop();
  }, [reduced, mood, life]);

  // One-shot expressions.
  useEffect(() => {
    if (!emote) return;
    switch (emote) {
      case "wink":
        // The right eye becomes an arc (see arcR); a small tilt of the head.
        if (!reduced) animate(hop, [0, -0.8, 0], { duration: 0.4 });
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
      case "star":
        break;
      case "sad": {
        // Lids slope down to the outer corners, the eyes sink a little.
        const ease = { duration: 0.35, ease: "easeOut" as const };
        animate(lidL, [0, 0.75, 0.75, 0], { duration: 2.1, times: [0, 0.15, 0.85, 1] });
        animate(lidR, [0, 0.75, 0.75, 0], { duration: 2.1, times: [0, 0.15, 0.85, 1] });
        animate(tiltL, -18, ease);
        animate(tiltR, 18, ease);
        animate(glanceY, [0, 1.6, 1.6, 0], { duration: 2.1, times: [0, 0.15, 0.85, 1] });
        window.setTimeout(() => {
          animate(tiltL, 0, ease);
          animate(tiltR, 0, ease);
        }, 1900);
        break;
      }
      case "determined": {
        const ease = { duration: 0.25, ease: "easeOut" as const };
        animate(lidL, [0, 0.6, 0.6, 0], { duration: 1.4, times: [0, 0.15, 0.85, 1] });
        animate(lidR, [0, 0.6, 0.6, 0], { duration: 1.4, times: [0, 0.15, 0.85, 1] });
        animate(tiltL, 20, ease);
        animate(tiltR, -20, ease);
        if (!reduced) animate(hop, [0, 1.2, 0], { duration: 0.35, delay: 0.2 });
        window.setTimeout(() => {
          animate(tiltL, 0, ease);
          animate(tiltR, 0, ease);
        }, 1250);
        break;
      }
      case "squint":
        // «Хм…»: both lids come down flat, the right one lower and slanted.
        animate(lidL, [0, 0.42, 0.42, 0], { duration: 1.3, times: [0, 0.2, 0.8, 1] });
        animate(lidR, [0, 0.72, 0.72, 0], { duration: 1.3, times: [0, 0.2, 0.8, 1] });
        animate(tiltR, [0, -9, -9, 0], { duration: 1.3, times: [0, 0.2, 0.8, 1] });
        break;
      case "confused":
        animate(sizeL, [1, 1.3, 1.3, 1], { duration: 1.7, times: [0, 0.2, 0.85, 1] });
        animate(sizeR, [1, 0.62, 0.62, 1], { duration: 1.7, times: [0, 0.2, 0.85, 1] });
        if (!reduced) animate(drift, [0, -1.2, 1.2, -1.2, 0], { duration: 1.7 });
        break;
      case "dizzy":
        if (!reduced) {
          // Two loops around: the eyes orbit their place.
          const r = 2.4;
          const n = 24;
          const xs = Array.from({ length: n + 1 }, (_, k) => r * Math.cos((k / n) * 4 * Math.PI) - (k === 0 || k === n ? r : 0));
          const ys = Array.from({ length: n + 1 }, (_, k) => r * Math.sin((k / n) * 4 * Math.PI));
          animate(drift, [...xs.slice(0, -1), 0], { duration: 1.3, ease: "linear" });
          animate(glanceY, [...ys.slice(0, -1), 0], { duration: 1.3, ease: "linear" });
        }
        break;
      case "yawn":
        // Open wide, then sink almost shut, slowly; then rest.
        animate(blinkL, [1, 1.25, 1.25, 0.12, 0.12, 1], { duration: 1.9, times: [0, 0.2, 0.4, 0.65, 0.85, 1] });
        animate(blinkR, [1, 1.25, 1.25, 0.12, 0.12, 1], { duration: 1.9, times: [0, 0.2, 0.4, 0.65, 0.85, 1] });
        break;
      case "wake":
        // From a line to open, with a little overshoot and a blink.
        animate(blinkL, [0.05, 0.05, 1.15, 1, 0.1, 1], { duration: 0.75, times: [0, 0.1, 0.45, 0.6, 0.78, 1] });
        animate(blinkR, [0.05, 0.05, 1.15, 1, 0.1, 1], { duration: 0.75, times: [0, 0.1, 0.45, 0.6, 0.78, 1] });
        break;
    }
  }, [emote, emoteSeq]);

  const fill = `url(#${ids.fill})`;
  const showArcs = mood === "happy" || emote === "laugh" || emote === "boop";
  // A wink closes one eye into a smiling arc; the other stays open.
  const arcL = showArcs;
  const arcR = showArcs || emote === "wink";
  const showHearts = emote === "heart";
  const showStars = emote === "star";
  const split = mood === "thinking";
  const pillsHidden = showHearts || showStars;

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
        <linearGradient id={ids.fill} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="var(--amber-hi)" />
          <stop offset="0.5" stopColor="var(--amber)" />
          <stop offset="1" stopColor="var(--amber-lo)" />
        </linearGradient>
        {/* Side shading: lit from the upper left, so the pill reads round. */}
        <linearGradient id={ids.shade} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#fff" stopOpacity="0.2" />
          <stop offset="0.38" stopColor="#fff" stopOpacity="0" />
          <stop offset="0.66" stopColor="#000" stopOpacity="0" />
          <stop offset="1" stopColor="#000" stopOpacity="0.3" />
        </linearGradient>
        <linearGradient id={ids.core} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#fff7e2" stopOpacity="0.95" />
          <stop offset="0.6" stopColor="#ffe3a8" stopOpacity="0.55" />
          <stop offset="1" stopColor="var(--amber-hi)" stopOpacity="0" />
        </linearGradient>
        <radialGradient id={ids.refl}>
          <stop offset="0" stopColor="var(--amber)" stopOpacity="0.32" />
          <stop offset="1" stopColor="var(--amber)" stopOpacity="0" />
        </radialGradient>
        <clipPath id={clipL}>
          <motion.path d={lidPathL} />
        </clipPath>
        <clipPath id={clipR}>
          <motion.path d={lidPathR} />
        </clipPath>
        {/* Two-step halo: a tight bright one and a wide soft one. */}
        <filter id={ids.glow} x="-300%" y="-150%" width="700%" height="400%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="1.1" result="tight" />
          <feGaussianBlur in="SourceGraphic" stdDeviation="3.6" result="wide" />
          <feComponentTransfer in="wide" result="soft">
            <feFuncA type="linear" slope="0.55" />
          </feComponentTransfer>
          <feMerge>
            <feMergeNode in="soft" />
            <feMergeNode in="tight" />
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

      <motion.g initial={false} animate={{ opacity: pillsHidden || mood === "sleep" ? 0 : 1 }} transition={{ duration: 0.18 }}>
        <Reflection x={leftX} y={yL} h={hL} ids={ids} />
        <Reflection x={rightX} y={yR} h={hR} ids={ids} />
      </motion.g>

      <motion.g initial={false} animate={{ opacity: pillsHidden ? 0 : mood === "sleep" ? 0.45 : 1 }} transition={{ duration: 0.18 }}>
        <motion.g style={{ opacity: life }}>
          <motion.g initial={false} animate={{ opacity: arcL ? 0 : 1 }} transition={{ duration: 0.12 }}>
            <Pill x={leftX} y={yL} h={hL} gx={gx} ids={ids} error={mood === "error"} clip={clipL} />
          </motion.g>
          <motion.g initial={false} animate={{ opacity: arcR ? 0 : 1 }} transition={{ duration: 0.12 }}>
            <Pill x={rightX} y={yR} h={hR} gx={gx} ids={ids} error={mood === "error"} clip={clipR} />
          </motion.g>
        </motion.g>
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={showStars ? { opacity: 1, scale: [0.3, 1.3, 0.95, 1.1, 1], rotate: [0, 45, 90, 90, 90] } : { opacity: 0, scale: 0.5, rotate: 0 }}
        transition={showStars ? { duration: 1, times: [0, 0.35, 0.55, 0.8, 1] } : { duration: 0.2 }}
        style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        fill={fill}
      >
        <path d={star(LEFT_CX)} />
        <path d={star(RIGHT_CX)} />
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={{ opacity: arcL || arcR ? 1 : 0, y: arcL || arcR ? 0 : 2 }}
        transition={{ duration: 0.22 }}
        style={{ y: hop }}
        stroke={fill}
        strokeWidth={2.8}
        strokeLinecap="round"
        fill="none"
      >
        <motion.path d={arc(LEFT_CX)} initial={false} animate={{ opacity: arcL ? 1 : 0 }} transition={{ duration: 0.15 }} />
        <motion.path d={arc(RIGHT_CX)} initial={false} animate={{ opacity: arcR ? 1 : 0 }} transition={{ duration: 0.15 }} />
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={showHearts ? { opacity: 1, scale: [0.4, 1.25, 1, 1.12, 1] } : { opacity: 0, scale: 0.6 }}
        transition={showHearts ? { duration: 0.9, times: [0, 0.3, 0.5, 0.75, 1] } : { duration: 0.2 }}
        style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        fill={fill}
      >
        <path d={heart(LEFT_CX)} />
        <path d={heart(RIGHT_CX)} />
      </motion.g>
    </svg>
  );
}
