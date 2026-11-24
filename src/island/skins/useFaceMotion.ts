// Shared motion state for all eye skins: moods, audio reactivity, gaze,
// blinking, idle glances, habits, one-shot emotes and tool-activity
// micro-animations. Idle skins pay zero frame cost between blinks.

import { animate, useMotionValue, useReducedMotion, useSpring, useTransform, type MotionValue } from "motion/react";
import { useEffect, useRef } from "react";
import { usePrefs } from "../../store/prefs";
import type { EyeMood, EyesProps } from "./types";

export const EYE_W = 6.4;
export const EYE_GAP = 14.5;
export const BOX_W = 44;
export const BOX_H = 30;
export const LEFT_CX = BOX_W / 2 - EYE_GAP / 2;
export const RIGHT_CX = BOX_W / 2 + EYE_GAP / 2;

export const HEIGHT_FOR: Record<EyeMood, number> = {
  idle: 13,
  attentive: 14.6,
  thinking: 7.5,
  listening: 9.6,
  speaking: 9.6,
  happy: 13,
  error: 3.2,
  sleep: 2.4,
};

export interface FaceMotionOptions {
  /** Scales glance/hop/drift amplitudes and softens springs (1 = amber default). */
  calmness?: number;
}

export interface FaceMotion {
  reduced: boolean;
  lvl: MotionValue<number>;
  life: MotionValue<number>;
  baseH: MotionValue<number>;
  blinkL: MotionValue<number>;
  blinkR: MotionValue<number>;
  widen: MotionValue<number>;
  drift: MotionValue<number>;
  glanceY: MotionValue<number>;
  hop: MotionValue<number>;
  sizeL: MotionValue<number>;
  sizeR: MotionValue<number>;
  lidL: MotionValue<number>;
  lidR: MotionValue<number>;
  tiltL: MotionValue<number>;
  tiltR: MotionValue<number>;
  gx: MotionValue<number>;
  lift: MotionValue<number>;
  hL: MotionValue<number>;
  hR: MotionValue<number>;
  yL: MotionValue<number>;
  yR: MotionValue<number>;
  leftX: MotionValue<number>;
  rightX: MotionValue<number>;
  showArcs: boolean;
  arcL: boolean;
  arcR: boolean;
  showHearts: boolean;
  showStars: boolean;
  pillsHidden: boolean;
}

/** The part of an eye left open by its lid: everything below a line through
 *  the eye's centre column, `drop` (0 open ... 1 half shut) down from its top,
 *  tilted by `tilt` degrees. Applied as a clip, so the glow follows the cut. */
export function lidClip(cx: number, top: number, h: number, drop: number, tilt: number): string {
  const X = 14;
  const line = top - 1.5 + drop * h * 0.55;
  const t = Math.tan((tilt * Math.PI) / 180);
  const bottom = top + h + 40;
  return `M${cx - X} ${line - t * X} L${cx + X} ${line + t * X} L${cx + X} ${bottom} L${cx - X} ${bottom} Z`;
}

export function useFaceMotion(props: EyesProps, options: FaceMotionOptions = {}): FaceMotion {
  const {
    mood,
    level,
    lookX,
    lookY = 0,
    emote = null,
    emoteSeq = 0,
    bob = false,
    sleepy = false,
    watch = false,
    alarm = false,
    activity = null,
  } = props;

  const calmness = options.calmness ?? 1;
  const osReduced = useReducedMotion();
  const prefReduced = usePrefs((p) => p.reduceMotion);
  const reduced = Boolean(osReduced || prefReduced);

  const springCfg = reduced
    ? { stiffness: 1000, damping: 100, mass: 1 }
    : calmness < 1
      ? { stiffness: Math.round(420 * calmness), damping: Math.round(30 / Math.sqrt(calmness)), mass: 0.72 }
      : { stiffness: 420, damping: 30, mass: 0.6 };

  const zero = useMotionValue(0);
  const life = useMotionValue(1);
  const lvl = level ?? zero;
  const gaze = lookX ?? zero;

  const resting = mood === "idle" && sleepy ? 6.5 : HEIGHT_FOR[mood];
  const baseH = useSpring(resting, springCfg);
  const blinkL = useMotionValue(1);
  const blinkR = useMotionValue(1);
  const widen = useMotionValue(1);
  const drift = useMotionValue(0);
  const glanceY = useMotionValue(0);
  const hop = useMotionValue(0);
  const sizeL = useMotionValue(1);
  const sizeR = useMotionValue(1);
  const lidL = useMotionValue(0);
  const lidR = useMotionValue(0);
  const tiltL = useMotionValue(0);
  const tiltR = useMotionValue(0);
  const yOff = useSpring(lookY, springCfg);

  useEffect(() => {
    baseH.set(resting);
  }, [resting, baseH]);

  useEffect(() => {
    yOff.set(lookY);
  }, [lookY, yOff]);

  const audioDriven = mood === "listening" || mood === "speaking";
  const open = () => (baseH.get() + (audioDriven ? lvl.get() * 13 : 0)) * widen.get();
  const hL = useTransform(() => Math.max(1.6, open() * blinkL.get() * sizeL.get()));
  const hR = useTransform(() => Math.max(1.6, open() * blinkR.get() * sizeR.get()));
  const lift = useTransform(() => yOff.get() + glanceY.get() + hop.get());
  const yL = useTransform(() => BOX_H / 2 - hL.get() / 2 + lift.get());
  const yR = useTransform(() => BOX_H / 2 - hR.get() / 2 + lift.get());
  const gx = useTransform(() => gaze.get() * 2.4 * calmness + drift.get());
  const leftX = useTransform(() => LEFT_CX - EYE_W / 2 + gx.get());
  const rightX = useTransform(() => RIGHT_CX - EYE_W / 2 + gx.get());

  const blinkBoth = (twice = false, slow = false) => {
    const keys = twice ? [1, 0.1, 1, 0.1, 1] : [1, 0.08, 1];
    const duration = (twice ? 0.42 : 0.17) * (slow ? 2.2 : 1) * (calmness < 1 ? 1.15 : 1);
    animate(blinkL, keys, { duration, ease: "easeInOut" });
    animate(blinkR, keys, { duration, ease: "easeInOut" });
  };

  // Rare blinks on timers so idle costs nothing between blinks.
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    const canBlink = (mood === "idle" || mood === "attentive") && !activity;
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
  }, [mood, watch, sleepy, activity]);

  // Idle glances.
  useEffect(() => {
    if (mood !== "idle" || reduced || watch || bob || activity) return;
    let t: number | undefined;
    const schedule = () => {
      t = window.setTimeout(() => {
        if (document.visibilityState === "visible") {
          const side = (Math.random() < 0.5 ? -1 : 1) * (1.4 + Math.random() * 1.2) * calmness;
          const up = (Math.random() - 0.5) * 2 * calmness;
          animate(drift, [0, side, side, 0], { duration: 1.6, times: [0, 0.15, 0.8, 1], ease: "easeInOut" });
          animate(glanceY, [0, up, up, 0], { duration: 1.6, times: [0, 0.15, 0.8, 1], ease: "easeInOut" });
        }
        schedule();
      }, 7000 + Math.random() * 9000);
    };
    schedule();
    return () => window.clearTimeout(t);
  }, [mood, reduced, watch, bob, activity, calmness]);

  // Tool-activity micro-animations. Always stops previous runs and resets cleanly
  // so transitions between tools never stack noisily.
  useEffect(() => {
    if (!activity) return;
    const c = calmness;

    if (reduced) {
      // Static, clear posture under reduced motion.
      switch (activity) {
        case "search":
          drift.set(1.6 * c);
          glanceY.set(-1.6 * c);
          break;
        case "read":
          glanceY.set(1.5 * c);
          break;
        case "type":
          glanceY.set(1.9 * c);
          break;
        case "look":
          widen.set(1.16);
          break;
        case "calendar":
          drift.set(-1.7 * c);
          glanceY.set(-1.6 * c);
          break;
        case "memory":
          glanceY.set(-2.1 * c);
          break;
        case "code":
          lidL.set(0.32);
          lidR.set(0.32);
          break;
        case "system":
          lidL.set(0.24);
          lidR.set(0.24);
          break;
        default:
          break;
      }
      return () => {
        drift.set(0);
        glanceY.set(0);
        widen.set(1);
        hop.set(0);
        sizeL.set(1);
        sizeR.set(1);
        lidL.set(0);
        lidR.set(0);
        blinkL.set(1);
        blinkR.set(1);
      };
    }

    const runs: { stop: () => void }[] = [];
    const dur = (s: number) => (c < 1 ? s * 1.15 : s);

    switch (activity) {
      case "search":
        runs.push(
          animate(glanceY, -2.2 * c, { duration: 0.28 }),
          animate(drift, [1.2 * c, 2.5 * c, 1.2 * c], { duration: dur(1.4), ease: "easeInOut", repeat: Infinity }),
        );
        break;
      case "read":
        runs.push(
          animate(glanceY, 1.8 * c, { duration: 0.28 }),
          animate(drift, [-1.9 * c, 1.9 * c, -1.9 * c], {
            duration: dur(1.3),
            ease: "linear",
            repeat: Infinity,
            repeatType: "loop",
            times: [0, 0.85, 1],
          }),
        );
        break;
      case "type":
        runs.push(
          animate(glanceY, 2.2 * c, { duration: 0.24 }),
          animate(drift, [0, -0.7 * c, 0.55 * c, -0.35 * c, 0.8 * c, 0], { duration: dur(0.85), repeat: Infinity }),
          animate(blinkL, [1, 1, 0.45, 1, 1, 0.55, 1], { duration: dur(1.1), times: [0, 0.22, 0.3, 0.38, 0.72, 0.78, 0.86], repeat: Infinity }),
          animate(blinkR, [1, 1, 0.45, 1, 1, 0.55, 1], { duration: dur(1.1), times: [0, 0.22, 0.3, 0.38, 0.72, 0.78, 0.86], repeat: Infinity }),
        );
        break;
      case "click":
        runs.push(
          animate(widen, [1, 0.78, 1.24, 1, 1], { duration: dur(0.95), times: [0, 0.16, 0.34, 0.52, 1], ease: "easeOut", repeat: Infinity }),
          animate(hop, [0, 0.65 * c, -0.95 * c, 0, 0], { duration: dur(0.95), times: [0, 0.16, 0.34, 0.52, 1], ease: "easeOut", repeat: Infinity }),
        );
        break;
      case "look":
        runs.push(
          animate(widen, 1.22, { duration: 0.28 }),
          animate(drift, [-2.4 * c, 2.4 * c], { duration: dur(0.95), ease: "easeInOut", repeat: Infinity, repeatType: "reverse" }),
        );
        break;
      case "draw": {
        // Trace a smooth arc with the gaze.
        const n = 16;
        const xs = Array.from({ length: n + 1 }, (_, k) => -2.2 * c + (4.4 * c * k) / n);
        const ys = Array.from({ length: n + 1 }, (_, k) => (1.2 - 2.8 * Math.sin((k / n) * Math.PI)) * c);
        runs.push(
          animate(drift, [...xs, ...xs.slice(0, -1).reverse()], { duration: dur(1.8), ease: "easeInOut", repeat: Infinity }),
          animate(glanceY, [...ys, ...ys.slice(0, -1).reverse()], { duration: dur(1.8), ease: "easeInOut", repeat: Infinity }),
        );
        break;
      }
      case "files":
        runs.push(
          animate(drift, [-1.7 * c, -1.7 * c, 1.7 * c, 1.7 * c, -1.7 * c], {
            duration: dur(1.4),
            times: [0, 0.35, 0.5, 0.85, 1],
            ease: "easeInOut",
            repeat: Infinity,
          }),
          animate(hop, [0, -0.7 * c, 0, -0.7 * c, 0], {
            duration: dur(1.4),
            times: [0, 0.42, 0.52, 0.92, 1],
            ease: "easeOut",
            repeat: Infinity,
          }),
        );
        break;
      case "music":
        runs.push(
          animate(hop, [0, 1.25 * c, 0, -0.5 * c, 0], { duration: dur(0.68), ease: "easeInOut", repeat: Infinity }),
          animate(drift, [-0.85 * c, 0.85 * c], { duration: dur(1.36), ease: "easeInOut", repeat: Infinity, repeatType: "reverse" }),
        );
        break;
      case "calendar":
        runs.push(
          animate(drift, [-2.1 * c, -1.3 * c, -2.1 * c], { duration: dur(1.6), ease: "easeInOut", repeat: Infinity }),
          animate(glanceY, -2.0 * c, { duration: 0.28 }),
        );
        break;
      case "memory":
        runs.push(
          animate(glanceY, -2.6 * c, { duration: 0.32 }),
          animate(widen, 1.08, { duration: 0.3 }),
          animate(drift, [-1.1 * c, 1.1 * c], { duration: dur(1.8), ease: "easeInOut", repeat: Infinity, repeatType: "reverse" }),
        );
        break;
      case "time":
        runs.push(
          animate(drift, [-1.6 * c, -1.6 * c, 1.6 * c, 1.6 * c, -1.6 * c], {
            duration: dur(1.1),
            times: [0, 0.42, 0.5, 0.92, 1],
            ease: "easeInOut",
            repeat: Infinity,
          }),
        );
        break;
      case "system":
        runs.push(
          animate(lidL, 0.28, { duration: 0.24 }),
          animate(lidR, 0.28, { duration: 0.24 }),
          animate(drift, [-1.1 * c, 1.1 * c], { duration: dur(1.3), ease: "easeInOut", repeat: Infinity, repeatType: "reverse" }),
        );
        break;
      case "code":
        runs.push(
          animate(lidL, 0.36, { duration: 0.22 }),
          animate(lidR, 0.36, { duration: 0.22 }),
          animate(widen, 0.9, { duration: 0.22 }),
          animate(drift, [-2.2 * c, 2.2 * c, -2.2 * c], {
            duration: dur(1.05),
            times: [0, 0.82, 1],
            ease: "linear",
            repeat: Infinity,
          }),
        );
        break;
      case "connect":
        runs.push(
          animate(sizeL, [1, 1.24, 0.86, 1], { duration: dur(0.95), ease: "easeInOut", repeat: Infinity }),
          animate(sizeR, [1, 0.86, 1.24, 1], { duration: dur(0.95), ease: "easeInOut", repeat: Infinity }),
        );
        break;
      case "speak":
        runs.push(
          animate(widen, [0.96, 1.28, 1.04, 1.34, 0.96], { duration: dur(0.85), ease: "easeInOut", repeat: Infinity }),
          animate(hop, [0, -0.45 * c, 0, -0.3 * c, 0], { duration: dur(0.85), ease: "easeInOut", repeat: Infinity }),
        );
        break;
      case "work":
        runs.push(
          animate(drift, [-1.5 * c, 1.5 * c], { duration: dur(1.5), ease: "easeInOut", repeat: Infinity, repeatType: "reverse" }),
          animate(hop, [0, -0.45 * c, 0], { duration: dur(1.5), ease: "easeInOut", repeat: Infinity }),
        );
        break;
    }

    return () => {
      runs.forEach((r) => r.stop());
      animate(drift, 0, { duration: 0.22 });
      animate(glanceY, 0, { duration: 0.22 });
      animate(widen, 1, { duration: 0.22 });
      animate(hop, 0, { duration: 0.2 });
      animate(sizeL, 1, { duration: 0.2 });
      animate(sizeR, 1, { duration: 0.2 });
      animate(lidL, 0, { duration: 0.2 });
      animate(lidR, 0, { duration: 0.2 });
      animate(blinkL, 1, { duration: 0.15 });
      animate(blinkR, 1, { duration: 0.15 });
    };
  }, [activity, reduced, calmness]);

  // Thinking scans left and right between tools; a timer's last seconds hold gaze right.
  useEffect(() => {
    if (activity) return;
    if (mood === "thinking" && !reduced) {
      const span = 2.2 * calmness;
      const controls = animate(drift, [0, -span, span, 0], { duration: calmness < 1 ? 2.8 : 2.4, ease: "easeInOut", repeat: Infinity });
      return () => controls.stop();
    }
    animate(drift, watch ? 2.4 * calmness : 0, { duration: 0.25 });
  }, [mood, watch, drift, activity, reduced, calmness]);

  // Music nod or ringing alarm hop.
  useEffect(() => {
    if (activity) return;
    if (reduced || !(alarm || (bob && (mood === "idle" || mood === "attentive")))) {
      animate(hop, 0, { duration: 0.2 });
      return;
    }
    const controls = alarm
      ? animate(hop, [0, -2.4 * calmness, 0], { duration: 0.34, ease: "easeOut", repeat: Infinity, repeatDelay: 0.08 })
      : animate(hop, [0, 1.1 * calmness, 0], { duration: 0.56, ease: "easeInOut", repeat: Infinity });
    return () => controls.stop();
  }, [bob, alarm, mood, reduced, hop, activity, calmness]);

  // Error head shake.
  useEffect(() => {
    if (mood === "error" && !reduced) {
      const c = calmness;
      animate(drift, [0, -3 * c, 3 * c, -2.4 * c, 2.4 * c, -1 * c, 0], { duration: 0.55 });
    }
  }, [mood, reduced, drift, calmness]);

  // Subtle living shimmer while busy.
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
    const timers: number[] = [];
    const c = calmness;
    switch (emote) {
      case "wink":
        if (!reduced) animate(hop, [0, -0.8 * c, 0], { duration: 0.4 });
        break;
      case "surprised":
        animate(widen, [1, 1.4, 1.4, 1], { duration: 0.8, times: [0, 0.15, 0.7, 1], ease: "easeOut" });
        break;
      case "shake":
        if (!reduced) animate(drift, [0, -3 * c, 3 * c, -2.4 * c, 2.4 * c, -1 * c, 0], { duration: 0.55 });
        break;
      case "laugh":
      case "boop":
        if (!reduced) animate(hop, [0, -1.6 * c, 0, -1.6 * c, 0, -0.8 * c, 0], { duration: emote === "laugh" ? 0.9 : 0.6 });
        break;
      case "heart":
      case "star":
        break;
      case "sad": {
        const ease = { duration: 0.35, ease: "easeOut" as const };
        animate(lidL, [0, 0.75, 0.75, 0], { duration: 2.1, times: [0, 0.15, 0.85, 1] });
        animate(lidR, [0, 0.75, 0.75, 0], { duration: 2.1, times: [0, 0.15, 0.85, 1] });
        animate(tiltL, -18, ease);
        animate(tiltR, 18, ease);
        animate(glanceY, [0, 1.6 * c, 1.6 * c, 0], { duration: 2.1, times: [0, 0.15, 0.85, 1] });
        timers.push(
          window.setTimeout(() => {
            animate(tiltL, 0, ease);
            animate(tiltR, 0, ease);
          }, 1900),
        );
        break;
      }
      case "determined": {
        const ease = { duration: 0.25, ease: "easeOut" as const };
        animate(lidL, [0, 0.6, 0.6, 0], { duration: 1.4, times: [0, 0.15, 0.85, 1] });
        animate(lidR, [0, 0.6, 0.6, 0], { duration: 1.4, times: [0, 0.15, 0.85, 1] });
        animate(tiltL, 20, ease);
        animate(tiltR, -20, ease);
        if (!reduced) animate(hop, [0, 1.2 * c, 0], { duration: 0.35, delay: 0.2 });
        timers.push(
          window.setTimeout(() => {
            animate(tiltL, 0, ease);
            animate(tiltR, 0, ease);
          }, 1250),
        );
        break;
      }
      case "squint":
        animate(lidL, [0, 0.42, 0.42, 0], { duration: 1.3, times: [0, 0.2, 0.8, 1] });
        animate(lidR, [0, 0.72, 0.72, 0], { duration: 1.3, times: [0, 0.2, 0.8, 1] });
        animate(tiltR, [0, -9, -9, 0], { duration: 1.3, times: [0, 0.2, 0.8, 1] });
        break;
      case "confused":
        animate(sizeL, [1, 1.3, 1.3, 1], { duration: 1.7, times: [0, 0.2, 0.85, 1] });
        animate(sizeR, [1, 0.62, 0.62, 1], { duration: 1.7, times: [0, 0.2, 0.85, 1] });
        if (!reduced) animate(drift, [0, -1.2 * c, 1.2 * c, -1.2 * c, 0], { duration: 1.7 });
        break;
      case "dizzy":
        if (!reduced) {
          const r = 2.4 * c;
          const n = 24;
          const xs = Array.from({ length: n + 1 }, (_, k) => r * Math.cos((k / n) * 4 * Math.PI) - (k === 0 || k === n ? r : 0));
          const ys = Array.from({ length: n + 1 }, (_, k) => r * Math.sin((k / n) * 4 * Math.PI));
          animate(drift, [...xs.slice(0, -1), 0], { duration: 1.3, ease: "linear" });
          animate(glanceY, [...ys.slice(0, -1), 0], { duration: 1.3, ease: "linear" });
        }
        break;
      case "yawn":
        animate(blinkL, [1, 1.25, 1.25, 0.12, 0.12, 1], { duration: 1.9, times: [0, 0.2, 0.4, 0.65, 0.85, 1] });
        animate(blinkR, [1, 1.25, 1.25, 0.12, 0.12, 1], { duration: 1.9, times: [0, 0.2, 0.4, 0.65, 0.85, 1] });
        break;
      case "wake":
        animate(blinkL, [0.05, 0.05, 1.15, 1, 0.1, 1], { duration: 0.75, times: [0, 0.1, 0.45, 0.6, 0.78, 1] });
        animate(blinkR, [0.05, 0.05, 1.15, 1, 0.1, 1], { duration: 0.75, times: [0, 0.1, 0.45, 0.6, 0.78, 1] });
        break;
    }
    return () => {
      timers.forEach((id) => window.clearTimeout(id));
    };
  }, [emote, emoteSeq, reduced, calmness]);

  const showArcs = mood === "happy" || emote === "laugh" || emote === "boop";
  const arcL = showArcs;
  const arcR = showArcs || emote === "wink";
  const showHearts = emote === "heart";
  const showStars = emote === "star";
  const pillsHidden = showHearts || showStars;

  return {
    reduced,
    lvl,
    life,
    baseH,
    blinkL,
    blinkR,
    widen,
    drift,
    glanceY,
    hop,
    sizeL,
    sizeR,
    lidL,
    lidR,
    tiltL,
    tiltR,
    gx,
    lift,
    hL,
    hR,
    yL,
    yR,
    leftX,
    rightX,
    showArcs,
    arcL,
    arcR,
    showHearts,
    showStars,
    pillsHidden,
  };
}
