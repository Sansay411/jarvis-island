// «Лёд» — cool cyan/white glassy eyes with a crisp specular rim, frosted inner
// core, diamond glint and calmer, more composed motion.

import { motion, useTransform, type MotionValue } from "motion/react";
import { useId } from "react";
import { ActivityGlyph } from "./ActivityGlyph";
import type { EyesProps } from "./types";
import { BOX_H, BOX_W, EYE_W, LEFT_CX, RIGHT_CX, lidClip, useFaceMotion } from "./useFaceMotion";

interface IceIds {
  fill: string;
  frost: string;
  core: string;
  glow: string;
}

function IceLens({
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
  ids: IceIds;
  error: boolean;
  clip: string;
}) {
  const coreW = EYE_W * 0.34;
  const coreX = useTransform(() => x.get() + (EYE_W - coreW) / 2);
  const coreY = useTransform(() => y.get() + h.get() * 0.12 + 0.35);
  const coreH = useTransform(() => Math.max(0, h.get() * 0.74 - 0.7));
  const glintX = useTransform(() => x.get() + EYE_W * 0.34 - gx.get() * 0.18);
  const glintY = useTransform(() => y.get() + Math.min(2.6, h.get() * 0.22) + 0.2);
  const glintD = useTransform(() => {
    const hv = h.get();
    if (hv <= 4.4) return "";
    const cx = glintX.get();
    const cy = glintY.get();
    const r = Math.min(1.25, hv * 0.095);
    return `M${cx} ${cy - r} L${cx + r * 0.75} ${cy} L${cx} ${cy + r} L${cx - r * 0.75} ${cy} Z`;
  });

  return (
    <>
      <g filter={`url(#${ids.glow})`}>
        <g clipPath={`url(#${clip})`}>
          <motion.rect x={x} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill={error ? "var(--danger)" : `url(#${ids.fill})`} />
        </g>
      </g>
      <g clipPath={`url(#${clip})`}>
        <motion.rect x={x} y={y} width={EYE_W} height={h} rx={EYE_W / 2} fill={`url(#${ids.frost})`} />
        <motion.rect x={coreX} y={coreY} width={coreW} height={coreH} rx={coreW / 2} fill={error ? "#ffd8d2" : `url(#${ids.core})`} opacity={0.86} />
        {/* Crisp crystalline rim */}
        <motion.rect
          x={x}
          y={y}
          width={EYE_W}
          height={h}
          rx={EYE_W / 2}
          fill="none"
          stroke={error ? "rgba(255,215,205,0.65)" : "rgba(232,251,255,0.74)"}
          strokeWidth={0.62}
        />
        <motion.path d={glintD} fill="#ffffff" opacity={0.95} />
      </g>
    </>
  );
}

function arc(cx: number): string {
  return `M${cx - 4} ${BOX_H / 2 + 1.8} Q${cx} ${BOX_H / 2 - 4.2} ${cx + 4} ${BOX_H / 2 + 1.8}`;
}

function star(cx: number): string {
  const cy = BOX_H / 2;
  const r = 5.2;
  const i = 1.1;
  return `M${cx} ${cy - r} L${cx + i} ${cy - i} L${cx + r} ${cy} L${cx + i} ${cy + i} L${cx} ${cy + r} L${cx - i} ${cy + i} L${cx - r} ${cy} L${cx - i} ${cy - i} Z`;
}

function heart(cx: number): string {
  const cy = BOX_H / 2;
  return `M${cx} ${cy + 3.6} C${cx - 5.5} ${cy - 0.4} ${cx - 3.7} ${cy - 5.5} ${cx} ${cy - 2.3} C${cx + 3.7} ${cy - 5.5} ${cx + 5.5} ${cy - 0.4} ${cx} ${cy + 3.6} Z`;
}

export function IceSkin(props: EyesProps) {
  const { mood, scale = 1, activity = null, onBoop } = props;
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const ids: IceIds = {
    fill: `ice-fill-${uid}`,
    frost: `ice-frost-${uid}`,
    core: `ice-core-${uid}`,
    glow: `ice-glow-${uid}`,
  };
  const clipL = `ice-lid-l-${uid}`;
  const clipR = `ice-lid-r-${uid}`;

  // Calmer motion profile for «Лёд».
  const m = useFaceMotion(props, { calmness: 0.72 });
  const lidPathL = useTransform(() => lidClip(m.leftX.get() + EYE_W / 2, m.yL.get(), m.hL.get(), m.lidL.get(), m.tiltL.get()));
  const lidPathR = useTransform(() => lidClip(m.rightX.get() + EYE_W / 2, m.yR.get(), m.hR.get(), m.lidR.get(), m.tiltR.get()));
  const splitLeftX = useTransform(() => m.leftX.get() - 0.9);
  const splitRightX = useTransform(() => m.rightX.get() + 0.9);

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
          <stop offset="0" stopColor="#ecfdff" />
          <stop offset="0.45" stopColor="#7ce8ff" />
          <stop offset="1" stopColor="#26a9cf" />
        </linearGradient>
        <linearGradient id={ids.frost} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ffffff" stopOpacity="0.42" />
          <stop offset="0.42" stopColor="#ffffff" stopOpacity="0.06" />
          <stop offset="0.7" stopColor="#0a2d42" stopOpacity="0.05" />
          <stop offset="1" stopColor="#041c2b" stopOpacity="0.34" />
        </linearGradient>
        <linearGradient id={ids.core} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#ffffff" stopOpacity="0.96" />
          <stop offset="0.55" stopColor="#c7f6ff" stopOpacity="0.5" />
          <stop offset="1" stopColor="#7ce8ff" stopOpacity="0" />
        </linearGradient>
        <clipPath id={clipL}>
          <motion.path d={lidPathL} />
        </clipPath>
        <clipPath id={clipR}>
          <motion.path d={lidPathR} />
        </clipPath>
        <filter id={ids.glow} x="-300%" y="-150%" width="700%" height="400%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="0.95" result="tight" />
          <feGaussianBlur in="SourceGraphic" stdDeviation="3.1" result="wide" />
          <feComponentTransfer in="wide" result="soft">
            <feFuncA type="linear" slope="0.48" />
          </feComponentTransfer>
          <feMerge>
            <feMergeNode in="soft" />
            <feMergeNode in="tight" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {split && (
        <g opacity={0.44} style={{ mixBlendMode: "screen" }}>
          <motion.rect x={splitLeftX} y={m.yL} width={EYE_W} height={m.hL} rx={EYE_W / 2} fill="#4da6ff" />
          <motion.rect x={splitRightX} y={m.yR} width={EYE_W} height={m.hR} rx={EYE_W / 2} fill="#e8fbff" />
        </g>
      )}

      <motion.g initial={false} animate={{ opacity: m.pillsHidden ? 0 : mood === "sleep" ? 0.45 : 1 }} transition={{ duration: 0.2 }}>
        <motion.g style={{ opacity: m.life }}>
          <motion.g initial={false} animate={{ opacity: m.arcL ? 0 : 1 }} transition={{ duration: 0.14 }}>
            <IceLens x={m.leftX} y={m.yL} h={m.hL} gx={m.gx} ids={ids} error={mood === "error"} clip={clipL} />
          </motion.g>
          <motion.g initial={false} animate={{ opacity: m.arcR ? 0 : 1 }} transition={{ duration: 0.14 }}>
            <IceLens x={m.rightX} y={m.yR} h={m.hR} gx={m.gx} ids={ids} error={mood === "error"} clip={clipR} />
          </motion.g>
        </motion.g>
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={m.showStars ? { opacity: 1, scale: [0.4, 1.2, 0.98, 1.06, 1], rotate: [0, 45, 90, 90, 90] } : { opacity: 0, scale: 0.5, rotate: 0 }}
        transition={m.showStars ? { duration: 1.05, times: [0, 0.35, 0.55, 0.8, 1] } : { duration: 0.2 }}
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
        transition={{ duration: 0.24 }}
        style={{ y: m.hop }}
        stroke={fill}
        strokeWidth={2.5}
        strokeLinecap="round"
        fill="none"
      >
        <motion.path d={arc(LEFT_CX)} initial={false} animate={{ opacity: m.arcL ? 1 : 0 }} transition={{ duration: 0.16 }} />
        <motion.path d={arc(RIGHT_CX)} initial={false} animate={{ opacity: m.arcR ? 1 : 0 }} transition={{ duration: 0.16 }} />
      </motion.g>

      <motion.g
        filter={`url(#${ids.glow})`}
        initial={false}
        animate={m.showHearts ? { opacity: 1, scale: [0.45, 1.18, 1, 1.08, 1] } : { opacity: 0, scale: 0.6 }}
        transition={m.showHearts ? { duration: 0.95, times: [0, 0.3, 0.5, 0.75, 1] } : { duration: 0.2 }}
        style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        fill={fill}
      >
        <path d={heart(LEFT_CX)} />
        <path d={heart(RIGHT_CX)} />
      </motion.g>

      <ActivityGlyph activity={activity} color="#c7f6ff" variant="soft" reduced={m.reduced} scale={scale} />
    </svg>
  );
}
