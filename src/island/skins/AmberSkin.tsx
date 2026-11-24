// «Янтарь» — JARVIS's own face and the default look. Two amber pills that
// blink on rare timers, glance around now and then while idle, widen when you
// approach, narrow and scan while thinking, follow real audio levels while
// listening/speaking, curve into arcs on success and flatten on errors. The
// motion itself lives in `useFaceMotion` (shared by every look).
//
// Each eye is a small lens of light, drawn in layers: a two-step halo, the
// amber body with side shading (so it reads round), a hot inner core, a thin
// light rim and a glint that drifts against the gaze. A very slow shimmer
// keeps it alive.

import { motion, useTransform, type MotionValue } from "motion/react";
import { useId } from "react";
import { ActivityGlyph } from "./ActivityGlyph";
import type { EyesProps } from "./types";
import { BOX_H, BOX_W, EYE_W, LEFT_CX, RIGHT_CX, lidClip, useFaceMotion } from "./useFaceMotion";

interface Ids {
  fill: string;
  shade: string;
  core: string;
  glow: string;
  refl: string;
}

/** One eye, layer by layer (see the header); `clip` is its lid. */
function Pill({
  x,
  y,
  h,
  gx,
  ids,
  error,
  clip,
}: {
  x: MotionValue<number>;
  y: MotionValue<number>;
  h: MotionValue<number>;
  gx: MotionValue<number>;
  ids: Ids;
  error: boolean;
  clip: string;
}) {
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
        <motion.rect
          x={x}
          y={y}
          width={EYE_W}
          height={h}
          rx={EYE_W / 2}
          fill="none"
          stroke={error ? "rgba(255,200,190,0.4)" : "rgba(255,240,212,0.38)"}
          strokeWidth={0.45}
        />
        <motion.circle cx={glintX} cy={glintY} r={glintR} fill="#fffaf0" opacity={0.92} />
      </g>
    </>
  );
}

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

export function AmberSkin(props: EyesProps) {
  const { mood, scale = 1, activity = null, onBoop } = props;
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const ids: Ids = {
    fill: `eye-fill-${uid}`,
    shade: `eye-shade-${uid}`,
    core: `eye-core-${uid}`,
    glow: `eye-glow-${uid}`,
    refl: `eye-refl-${uid}`,
  };
  const clipL = `eye-lid-l-${uid}`;
  const clipR = `eye-lid-r-${uid}`;

  const m = useFaceMotion(props);
  const lidPathL = useTransform(() => lidClip(m.leftX.get() + EYE_W / 2, m.yL.get(), m.hL.get(), m.lidL.get(), m.tiltL.get()));
  const lidPathR = useTransform(() => lidClip(m.rightX.get() + EYE_W / 2, m.yR.get(), m.hR.get(), m.lidR.get(), m.tiltR.get()));
  const splitLeftX = useTransform(() => m.leftX.get() - 1.1);
  const splitRightX = useTransform(() => m.rightX.get() + 1.1);

  const fill = `url(#${ids.fill})`;
  const split = mood === "thinking";

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
          <motion.rect x={splitLeftX} y={m.yL} width={EYE_W} height={m.hL} rx={EYE_W / 2} fill="#62d6ff" />
          <motion.rect x={splitRightX} y={m.yR} width={EYE_W} height={m.hR} rx={EYE_W / 2} fill="#ff6b8b" />
        </g>
      )}

      <motion.g initial={false} animate={{ opacity: m.pillsHidden ? 0 : mood === "sleep" ? 0.45 : 1 }} transition={{ duration: 0.18 }}>
        <motion.g style={{ opacity: m.life }}>
          <motion.g initial={false} animate={{ opacity: m.arcL ? 0 : 1 }} transition={{ duration: 0.12 }}>
            <Pill x={m.leftX} y={m.yL} h={m.hL} gx={m.gx} ids={ids} error={mood === "error"} clip={clipL} />
          </motion.g>
          <motion.g initial={false} animate={{ opacity: m.arcR ? 0 : 1 }} transition={{ duration: 0.12 }}>
            <Pill x={m.rightX} y={m.yR} h={m.hR} gx={m.gx} ids={ids} error={mood === "error"} clip={clipR} />
          </motion.g>
        </motion.g>
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={m.showStars ? { opacity: 1, scale: [0.3, 1.3, 0.95, 1.1, 1], rotate: [0, 45, 90, 90, 90] } : { opacity: 0, scale: 0.5, rotate: 0 }}
        transition={m.showStars ? { duration: 1, times: [0, 0.35, 0.55, 0.8, 1] } : { duration: 0.2 }}
        style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        fill={fill}
      >
        <path d={star(LEFT_CX)} />
        <path d={star(RIGHT_CX)} />
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={{ opacity: m.arcL || m.arcR ? 1 : 0, y: m.arcL || m.arcR ? 0 : 2 }}
        transition={{ duration: 0.22 }}
        style={{ y: m.hop }}
        stroke={fill}
        strokeWidth={2.8}
        strokeLinecap="round"
        fill="none"
      >
        <motion.path d={arc(LEFT_CX)} initial={false} animate={{ opacity: m.arcL ? 1 : 0 }} transition={{ duration: 0.15 }} />
        <motion.path d={arc(RIGHT_CX)} initial={false} animate={{ opacity: m.arcR ? 1 : 0 }} transition={{ duration: 0.15 }} />
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={m.showHearts ? { opacity: 1, scale: [0.4, 1.25, 1, 1.12, 1] } : { opacity: 0, scale: 0.6 }}
        transition={m.showHearts ? { duration: 0.9, times: [0, 0.3, 0.5, 0.75, 1] } : { duration: 0.2 }}
        style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        fill={fill}
      >
        <path d={heart(LEFT_CX)} />
        <path d={heart(RIGHT_CX)} />
      </motion.g>

      <ActivityGlyph activity={activity} color="var(--amber-hi)" variant="soft" reduced={m.reduced} scale={scale} />
    </svg>
  );
}
