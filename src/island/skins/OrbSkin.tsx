// «Сфера» — a single glowing orb made of small light dots that ripples with
// voice levels, condenses while thinking, scatters slightly on error, smiles
// as an arc of dots when happy, and forms dot-constellations for emotes.
// Runs zero RAF ticks when resting idle.

import { useEffect, useId, useRef } from "react";
import { ActivityGlyph } from "./ActivityGlyph";
import type { EyesProps } from "./types";
import { BOX_H, BOX_W, useFaceMotion } from "./useFaceMotion";

interface DotSeed {
  ring: number; // 0 = center, 1 = inner, 2 = mid, 3 = outer
  angle: number;
  r: number;
  z: number; // 0.35..1 depth factor for parallax & brightness
  size: number;
  scatterA: number;
  scatterD: number;
}

const CX = BOX_W / 2;
const CY = BOX_H / 2;

const DOTS: DotSeed[] = (() => {
  const list: DotSeed[] = [
    { ring: 0, angle: 0, r: 0, z: 1, size: 1.55, scatterA: 0.4, scatterD: 1.2 },
  ];
  const rings: [number, number, number][] = [
    [1, 6, 3.3],
    [2, 10, 6.4],
    [3, 14, 9.3],
  ];
  let idx = 1;
  for (const [ring, count, radius] of rings) {
    for (let i = 0; i < count; i++) {
      const angle = (i / count) * Math.PI * 2 + ring * 0.42;
      const z = Math.sqrt(Math.max(0.18, 1 - (radius / 10.6) ** 2));
      const scatterA = angle + ((idx * 1.618) % 1 - 0.5) * 0.9;
      const scatterD = 1.6 + ((idx * 7) % 5) * 0.72;
      list.push({
        ring,
        angle,
        r: radius,
        z,
        size: ring === 1 ? 1.28 : ring === 2 ? 1.05 : 0.82,
        scatterA,
        scatterD,
      });
      idx++;
    }
  }
  return list;
})();

const DOT_COUNT = DOTS.length;

function arcPoint(i: number, total: number): [number, number] {
  const t = total > 1 ? i / (total - 1) : 0.5;
  // Two subtle layered rows along a smiling upward curve.
  const row = i % 2 === 0 ? 0 : 1;
  const u = (t - 0.5) * 2; // -1..1
  const x = u * 11.2;
  const y = 3.2 - (1 - u * u) * 7.2 + (row ? 1.65 : -0.45);
  return [x, y];
}

function heartPoint(i: number, total: number): [number, number] {
  // Parametric heart distributed along perimeter + inner echo.
  const inner = i < 9;
  const t = inner ? (i / 9) * Math.PI * 2 : ((i - 9) / Math.max(1, total - 9)) * Math.PI * 2;
  const s = inner ? 0.34 : 0.62;
  const x = 16 * Math.sin(t) ** 3 * s;
  const y = -(13 * Math.cos(t) - 5 * Math.cos(2 * t) - 2 * Math.cos(3 * t) - Math.cos(4 * t)) * s + 0.6;
  return [x, y];
}

function starPoint(i: number, total: number): [number, number] {
  if (i === 0) return [0, 0];
  const arm = (i - 1) % 4;
  const step = Math.floor((i - 1) / 4);
  const maxStep = Math.ceil((total - 1) / 4);
  const frac = (step + 1) / maxStep;
  const a = (arm * Math.PI) / 2;
  // Alternate points sit slightly off-axis near the center so the star reads full.
  const diag = step % 2 === 1 && frac < 0.65;
  const theta = diag ? a + Math.PI / 4 : a;
  const rad = diag ? frac * 5.2 : frac * 10.6;
  return [Math.cos(theta) * rad, Math.sin(theta) * rad];
}

export function OrbSkin(props: EyesProps) {
  const { mood, scale = 1, emote = null, bob = false, alarm = false, activity = null, onBoop } = props;
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const glowId = `orb-glow-${uid}`;
  const coreId = `orb-core-${uid}`;

  const m = useFaceMotion(props, { calmness: 0.9 });
  const dotsRef = useRef<(SVGCircleElement | null)[]>([]);
  const haloRef = useRef<SVGCircleElement | null>(null);
  const phaseRef = useRef(0);

  const propsRef = useRef({ mood, emote, activity, reduced: m.reduced });
  propsRef.current = { mood, emote, activity, reduced: m.reduced };

  const renderFrame = (phase: number) => {
    const { mood: curMood, emote: curEmote, activity: curAct, reduced } = propsRef.current;
    const gx = m.gx.get();
    const lift = m.lift.get();
    const lvl = m.lvl.get();
    const blink = (m.blinkL.get() + m.blinkR.get()) * 0.5;
    const widen = m.widen.get();
    const sizeAsym = m.sizeL.get() - m.sizeR.get();
    const lidAvg = (m.lidL.get() + m.lidR.get()) * 0.5;
    const tiltAvg = (m.tiltL.get() - m.tiltR.get()) * 0.5; // positive = determined, negative = sad
    const life = m.life.get();

    const isError = curMood === "error";
    const isSleep = curMood === "sleep";
    const isThinking = curMood === "thinking";
    const isVoice = curMood === "listening" || curMood === "speaking";
    const isHappyArc = curMood === "happy" || curEmote === "laugh" || curEmote === "boop";
    const isHeart = curEmote === "heart";
    const isStar = curEmote === "star";
    const isWink = curEmote === "wink";

    const color = isError ? "var(--danger)" : "#fffdf6";
    const outerColor = isError ? "#ff8a7a" : "#f3e4c6";

    if (haloRef.current) {
      const hr = isSleep ? 6.5 : isThinking ? 7.8 : isVoice ? 10.2 + lvl * 4.5 : 9.8 * widen;
      haloRef.current.setAttribute("cx", (CX + gx * 0.55).toFixed(2));
      haloRef.current.setAttribute("cy", (CY + lift * 0.55).toFixed(2));
      haloRef.current.setAttribute("r", Math.max(4, hr).toFixed(2));
      haloRef.current.setAttribute("opacity", (isSleep ? 0.16 : isError ? 0.34 : 0.28 + lvl * 0.25).toFixed(2));
    }

    for (let i = 0; i < DOT_COUNT; i++) {
      const el = dotsRef.current[i];
      if (!el) continue;
      const d = DOTS[i]!;

      let dx: number;
      let dy: number;
      let rSize = d.size;
      let op = (0.45 + 0.55 * d.z) * life;

      if (isHappyArc) {
        const [ax, ay] = arcPoint(i, DOT_COUNT);
        dx = ax + gx * 0.45;
        dy = ay + lift * 0.7;
        rSize = d.ring <= 1 ? 1.25 : 1.0;
        op = 0.92;
      } else if (isHeart) {
        const [hx, hy] = heartPoint(i, DOT_COUNT);
        const pulse = reduced ? 1 : 1 + 0.08 * Math.sin(phase * 4);
        dx = hx * pulse + gx * 0.35;
        dy = hy * pulse + lift * 0.5;
        rSize = i < 9 ? 0.95 : 1.15;
        op = 0.95;
      } else if (isStar) {
        const [sx, sy] = starPoint(i, DOT_COUNT);
        const rot = reduced ? 0 : phase * 0.8;
        const c = Math.cos(rot);
        const s = Math.sin(rot);
        dx = sx * c - sy * s + gx * 0.35;
        dy = sx * s + sy * c + lift * 0.5;
        rSize = i === 0 ? 1.55 : 1.05;
        op = 0.96;
      } else {
        // Spherical constellation with dynamic radial / angular modulation.
        const spin = reduced
          ? 0
          : isThinking
            ? phase * (d.ring === 2 ? -1.2 : 1.45)
            : curAct === "draw"
              ? phase * 1.1
              : curAct === "code"
                ? phase * 0.9
                : 0;

        let rad = d.r * widen;
        if (isThinking && !curAct) {
          rad *= 0.64 + (reduced ? 0 : 0.06 * Math.sin(phase * 2.4 + d.ring));
        } else if (isVoice) {
          // Concentric voice ripple traveling outward from center.
          const wave = reduced ? lvl * 2.6 : Math.sin(phase * 5.2 - d.ring * 1.15) * (0.45 + lvl * 2.4) + lvl * 3.1;
          rad = Math.max(0, d.r + wave * (0.35 + d.ring * 0.25));
          rSize = d.size * (1 + lvl * 0.38);
        } else if (curAct === "connect") {
          const wave = reduced ? 0 : Math.sin(phase * 4.5 - d.ring * 1.2) * 1.25;
          rad = Math.max(0, d.r * 0.92 + wave);
        } else if (curAct === "speak") {
          const wave = reduced ? 0 : Math.sin(phase * 6 - d.ring) * 1.5;
          rad = Math.max(0, d.r + wave);
        } else if (curAct === "code") {
          rad *= 0.78;
        }

        const theta = d.angle + spin;
        dx = Math.cos(theta) * rad;
        dy = Math.sin(theta) * rad;

        // Blink & sleep flatten vertically.
        const flatY = isSleep ? 0.16 : Math.max(0.12, blink);
        dy *= flatY;

        // Wink curls the right hemisphere slightly upward.
        if (isWink && dx > 1) {
          dy = dy * 0.22 - 1.4 * Math.sin(Math.min(1, dx / 9.5) * Math.PI);
        }

        // Confusion tilts asymmetry across left/right halves.
        if (Math.abs(sizeAsym) > 0.05) {
          const sideScale = dx < 0 ? 1 + sizeAsym * 0.35 : 1 - sizeAsym * 0.35;
          dy *= Math.max(0.45, sideScale);
        }

        // Lids/tilts (sad droops outer edges, determined pinches top center).
        if (lidAvg > 0.05) {
          if (dy < 0) dy *= 1 - lidAvg * 0.65;
          if (tiltAvg < -5) dy += (Math.abs(dx) / 9.5) * 2.2 * lidAvg;
          else if (tiltAvg > 5) dy -= (Math.abs(dx) / 9.5) * 1.8 * lidAvg;
        }

        // Error scatters dots slightly outward.
        if (isError) {
          const jitter = reduced ? 1 : 0.85 + 0.25 * Math.sin(phase * 9 + i);
          dx += Math.cos(d.scatterA) * d.scatterD * jitter;
          dy += Math.sin(d.scatterA) * d.scatterD * jitter * 0.55;
        }

        // 3D spherical parallax: closer (inner) dots track gaze more strongly.
        const parallax = 0.55 + d.z * 0.75;
        dx += gx * parallax;
        dy += lift * parallax;

        if (isSleep) op *= 0.45;
      }

      el.setAttribute("cx", (CX + dx).toFixed(2));
      el.setAttribute("cy", (CY + dy).toFixed(2));
      el.setAttribute("r", Math.max(0.45, rSize).toFixed(2));
      el.setAttribute("fill", d.ring <= 1 ? color : outerColor);
      el.setAttribute("opacity", Math.max(0.15, Math.min(1, op)).toFixed(2));
    }
  };

  // Subscribe to MotionValues so blinks, gaze and emotes update without needing an idle RAF loop.
  useEffect(() => {
    renderFrame(phaseRef.current);
    const values = [m.gx, m.lift, m.lvl, m.blinkL, m.blinkR, m.widen, m.sizeL, m.sizeR, m.lidL, m.lidR, m.tiltL, m.tiltR, m.life];
    const unsubs = values.map((mv) => mv.on("change", () => renderFrame(phaseRef.current)));
    return () => unsubs.forEach((off) => off());
  }, [mood, emote, activity, m.reduced]);

  // Continuous gentle RAF only when actively busy (never while resting idle).
  useEffect(() => {
    const animated =
      !m.reduced &&
      (mood === "thinking" ||
        mood === "listening" ||
        mood === "speaking" ||
        mood === "error" ||
        Boolean(activity) ||
        emote === "heart" ||
        emote === "star" ||
        bob ||
        alarm);
    if (!animated) {
      renderFrame(phaseRef.current);
      return;
    }
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      phaseRef.current += dt;
      renderFrame(phaseRef.current);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [mood, emote, activity, bob, alarm, m.reduced]);

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
        <radialGradient id={coreId}>
          <stop offset="0" stopColor={mood === "error" ? "var(--danger)" : "#fffdf6"} stopOpacity="0.8" />
          <stop offset="0.55" stopColor={mood === "error" ? "var(--danger)" : "#f3e4c6"} stopOpacity="0.28" />
          <stop offset="1" stopColor={mood === "error" ? "var(--danger)" : "#f3e4c6"} stopOpacity="0" />
        </radialGradient>
        <filter id={glowId} x="-150%" y="-150%" width="400%" height="400%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="1.15" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <circle ref={haloRef} cx={CX} cy={CY} r={9.8} fill={`url(#${coreId})`} opacity={0.28} />
      <g filter={`url(#${glowId})`}>
        {DOTS.map((_, i) => (
          <circle
            key={i}
            ref={(el) => {
              dotsRef.current[i] = el;
            }}
            cx={CX}
            cy={CY}
            r={1}
            fill="#f3e4c6"
          />
        ))}
      </g>

      <ActivityGlyph activity={activity} color="#fff6e2" variant="soft" reduced={m.reduced} scale={scale} />
    </svg>
  );
}
