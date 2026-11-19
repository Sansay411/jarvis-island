// The focus dial: a 60-minute face with tick marks, the arc of what is left,
// a glowing head at the arc's end while focus runs, a ripple when a session
// starts, a shake on pause and an amber burst when a session is done.
// Every effect is one-shot (on an event); nothing loops while idle.

import { useEffect, useMemo, useRef, useState, type PointerEvent as RPointerEvent, type WheelEvent } from "react";
import { AnimatePresence, motion, useAnimate, useReducedMotion } from "motion/react";
import { FlipNumber } from "./FocusDeck";

const TICKS = 60;

export function FocusDial({
  size = 104,
  fraction,
  minutes,
  caption,
  live,
  paused,
  active,
  burstKey,
  onSetMinutes,
}: {
  size?: number;
  /** Arc length, 0..1. */
  fraction: number;
  /** Number in the centre; also how many ticks are lit. */
  minutes: number;
  caption: string;
  live: boolean;
  paused: boolean;
  active: boolean;
  /** Bump to fire the completion burst. */
  burstKey: number;
  /** Idle only: turning the dial (drag or wheel) sets 1..60 minutes. */
  onSetMinutes?: (m: number) => void;
}) {
  const [turning, setTurning] = useState(false);
  const minutesAt = (e: { clientX: number; clientY: number }, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    const dx = e.clientX - (r.left + r.width / 2);
    const dy = e.clientY - (r.top + r.height / 2);
    const deg = (Math.atan2(dx, -dy) * 180) / Math.PI;
    const m = Math.round(((deg + 360) % 360) / 6);
    return m === 0 ? 60 : m;
  };
  const turnHandlers = onSetMinutes
    ? {
        onPointerDown: (e: RPointerEvent<HTMLDivElement>) => {
          try {
            e.currentTarget.setPointerCapture(e.pointerId);
          } catch {
            /* synthetic or already-released pointer */
          }
          setTurning(true);
          onSetMinutes(minutesAt(e, e.currentTarget));
        },
        onPointerMove: (e: RPointerEvent<HTMLDivElement>) => {
          if (!turning) return;
          onSetMinutes(minutesAt(e, e.currentTarget));
        },
        onPointerUp: () => setTurning(false),
        onPointerCancel: () => setTurning(false),
        onWheel: (e: WheelEvent<HTMLDivElement>) => {
          e.stopPropagation();
          onSetMinutes(Math.min(60, Math.max(1, Math.round(minutes) + (e.deltaY < 0 ? 1 : -1))));
        },
      }
    : {};
  const reduced = useReducedMotion();
  const c = size / 2;
  const stroke = 5;
  const r = c - 16;
  const len = 2 * Math.PI * r;
  const f = Math.min(1, Math.max(0, fraction));
  const lit = Math.max(0, Math.min(TICKS, Math.round(minutes)));

  // Ripple once each time a session starts.
  const startKey = useRef(0);
  const wasActive = useRef(active);
  if (active && !wasActive.current) startKey.current += 1;
  wasActive.current = active;

  // Shake once on pause / resume.
  const [scope, animate] = useAnimate();
  const firstPause = useRef(true);
  useEffect(() => {
    if (firstPause.current) {
      firstPause.current = false;
      return;
    }
    if (reduced || !scope.current) return;
    void animate(scope.current, { x: [0, -4, 4, -2, 0] }, { duration: 0.38, ease: "easeOut" });
  }, [paused, animate, scope, reduced]);

  const sparks = useMemo(
    () =>
      Array.from({ length: 18 }, (_, i) => {
        const a = (i / 18) * Math.PI * 2 + (i % 3) * 0.17;
        const d = 58 + (i % 4) * 9;
        return { x: Math.cos(a) * d, y: Math.sin(a) * d, s: 4 + (i % 3), rot: (i % 2 ? 1 : -1) * (90 + i * 12), amber: i % 3 !== 0 };
      }),
    [],
  );

  return (
    <div
      ref={scope}
      className={`focus-dial${live ? " is-live" : ""}${paused ? " is-paused" : ""}${onSetMinutes ? " is-turnable" : ""}${turning ? " is-turning" : ""}`}
      style={{ width: size, height: size }}
      title={onSetMinutes ? "Покрутите, чтобы выбрать минуты" : undefined}
      {...turnHandlers}
    >
      <span className="focus-dial-glow" aria-hidden="true" />

      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="focus-dial-svg" aria-hidden="true">
        {Array.from({ length: TICKS }, (_, i) => {
          const a = (i / TICKS) * Math.PI * 2;
          const major = i % 5 === 0;
          const r1 = c - (major ? 9 : 7);
          const r2 = c - 3;
          return (
            <line
              key={i}
              x1={c + Math.cos(a) * r1}
              y1={c + Math.sin(a) * r1}
              x2={c + Math.cos(a) * r2}
              y2={c + Math.sin(a) * r2}
              className={`focus-tick${i < lit ? " is-lit" : ""}${major ? " is-major" : ""}`}
              style={{ transitionDelay: reduced || turning ? "0ms" : `${i * 7}ms` }}
            />
          );
        })}
        <circle cx={c} cy={c} r={r} strokeWidth={stroke} className="focus-dial-track" />
        <circle
          cx={c}
          cy={c}
          r={r}
          strokeWidth={stroke}
          strokeDasharray={len}
          strokeDashoffset={len * (1 - f)}
          className="focus-dial-arc"
        />
        {onSetMinutes && (
          <g className="focus-dial-knob" style={{ transform: `rotate(${f * 360}deg)` }}>
            <circle cx={c + r} cy={c} r={turning ? 6.5 : 5} className="focus-dial-knob-dot" />
          </g>
        )}
        {live && f > 0.01 && (
          <g className="focus-dial-head" style={{ transform: `rotate(${f * 360}deg)` }}>
            <circle cx={c + r} cy={c} r={7} className="focus-dial-head-halo" />
            <circle cx={c + r} cy={c} r={3.6} className="focus-dial-head-dot" />
          </g>
        )}
      </svg>

      <div className="focus-dial-center">
        <span className="focus-dial-num">
          <FlipNumber value={minutes} />
        </span>
        <span className="focus-dial-cap">{caption}</span>
      </div>

      {!reduced && (
        <AnimatePresence>
          {startKey.current > 0 && (
            <motion.span
              key={`ripple-${startKey.current}`}
              className="focus-dial-ripple"
              initial={{ scale: 0.7, opacity: 0.7 }}
              animate={{ scale: 1.7, opacity: 0 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.9, ease: [0.22, 1, 0.36, 1] }}
            />
          )}
        </AnimatePresence>
      )}

      {!reduced && burstKey > 0 && (
        <div key={`burst-${burstKey}`} className="focus-dial-burst" aria-hidden="true">
          {sparks.map((p, i) => (
            <motion.span
              key={i}
              className={`focus-spark${p.amber ? " is-amber" : ""}`}
              style={{ width: p.s, height: p.s }}
              initial={{ x: 0, y: 0, opacity: 1, scale: 1, rotate: 0 }}
              animate={{ x: p.x, y: p.y, opacity: 0, scale: 0.4, rotate: p.rot }}
              transition={{ duration: 0.95, ease: [0.16, 1, 0.3, 1], delay: (i % 4) * 0.02 }}
            />
          ))}
        </div>
      )}
    </div>
  );
}
