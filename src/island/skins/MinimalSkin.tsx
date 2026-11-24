// «Минимал» — two thin light lines/arcs in warm bone-white, ultra-calm and
// understated for users who prefer a quiet presence.

import { motion, useTransform, type MotionValue } from "motion/react";
import { useId } from "react";
import { ActivityGlyph } from "./ActivityGlyph";
import type { EyesProps } from "./types";
import { BOX_H, BOX_W, EYE_W, LEFT_CX, RIGHT_CX, useFaceMotion } from "./useFaceMotion";

function MinimalStroke({
  x,
  y,
  h,
  lid,
  tilt,
  flat,
  error,
}: {
  x: MotionValue<number>;
  y: MotionValue<number>;
  h: MotionValue<number>;
  lid: MotionValue<number>;
  tilt: MotionValue<number>;
  flat: boolean;
  error: boolean;
}) {
  const d = useTransform(() => {
    const cx = x.get() + EYE_W / 2;
    const hv = h.get();
    const cy = y.get() + hv / 2;
    const lidV = lid.get();
    const tiltV = tilt.get();

    // Sleep / error render as quiet horizontal dashes.
    if (flat || hv <= 3.6) {
      const half = flat ? 4.2 : 3.6;
      const dy = Math.tan((tiltV * Math.PI) / 180) * half * 0.35;
      return `M${(cx - half).toFixed(2)} ${(cy - dy).toFixed(2)} L${(cx + half).toFixed(2)} ${(cy + dy).toFixed(2)}`;
    }

    // Slanted eyebrow-like strokes during sad / determined / squint emotes.
    if (lidV > 0.25 && Math.abs(tiltV) > 4) {
      const halfX = 3.8;
      const rad = (tiltV * Math.PI) / 180;
      const dx = Math.cos(rad) * halfX;
      const dy = Math.sin(rad) * halfX;
      return `M${(cx - dx).toFixed(2)} ${(cy - dy).toFixed(2)} L${(cx + dx).toFixed(2)} ${(cy + dy).toFixed(2)}`;
    }

    const effectiveH = Math.max(2.2, hv * (1 - lidV * 0.48) * 0.86);
    const halfY = effectiveH / 2;
    return `M${cx.toFixed(2)} ${(cy - halfY).toFixed(2)} L${cx.toFixed(2)} ${(cy + halfY).toFixed(2)}`;
  });

  return (
    <motion.path
      d={d}
      stroke={error ? "var(--danger)" : "#ecebe6"}
      strokeWidth={2.0}
      strokeLinecap="round"
      fill="none"
    />
  );
}

function arc(cx: number): string {
  return `M${cx - 3.8} ${BOX_H / 2 + 1.4} Q${cx} ${BOX_H / 2 - 3.6} ${cx + 3.8} ${BOX_H / 2 + 1.4}`;
}

function star(cx: number): string {
  const cy = BOX_H / 2;
  const r = 4.5;
  const i = 1.1;
  return `M${cx} ${cy - r} Q${cx + i} ${cy - i} ${cx + r} ${cy} Q${cx + i} ${cy + i} ${cx} ${cy + r} Q${cx - i} ${cy + i} ${cx - r} ${cy} Q${cx - i} ${cy - i} ${cx} ${cy - r} Z`;
}

function heart(cx: number): string {
  const cy = BOX_H / 2;
  return `M${cx} ${cy + 3.2} C${cx - 4.8} ${cy - 0.3} ${cx - 3.2} ${cy - 4.8} ${cx} ${cy - 1.9} C${cx + 3.2} ${cy - 4.8} ${cx + 4.8} ${cy - 0.3} ${cx} ${cy + 3.2} Z`;
}

export function MinimalSkin(props: EyesProps) {
  const { mood, scale = 1, activity = null, onBoop } = props;
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const glowId = `min-glow-${uid}`;

  // Extra calm motion profile.
  const m = useFaceMotion(props, { calmness: 0.62 });
  const flat = mood === "sleep" || mood === "error";
  const error = mood === "error";

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
        <filter id={glowId} x="-150%" y="-150%" width="400%" height="400%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="1.4" result="blur" />
          <feComponentTransfer in="blur" result="soft">
            <feFuncA type="linear" slope="0.28" />
          </feComponentTransfer>
          <feMerge>
            <feMergeNode in="soft" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <g filter={`url(#${glowId})`}>
        <motion.g initial={false} animate={{ opacity: m.pillsHidden ? 0 : mood === "sleep" ? 0.42 : 0.94 }} transition={{ duration: 0.2 }}>
          <motion.g style={{ opacity: m.life }}>
            <motion.g initial={false} animate={{ opacity: m.arcL ? 0 : 1 }} transition={{ duration: 0.14 }}>
              <MinimalStroke x={m.leftX} y={m.yL} h={m.hL} lid={m.lidL} tilt={m.tiltL} flat={flat} error={error} />
            </motion.g>
            <motion.g initial={false} animate={{ opacity: m.arcR ? 0 : 1 }} transition={{ duration: 0.14 }}>
              <MinimalStroke x={m.rightX} y={m.yR} h={m.hR} lid={m.lidR} tilt={m.tiltR} flat={flat} error={error} />
            </motion.g>
          </motion.g>
        </motion.g>

        <motion.g
          initial={false}
          animate={{ opacity: m.arcL || m.arcR ? 0.95 : 0, y: m.arcL || m.arcR ? 0 : 1.5 }}
          transition={{ duration: 0.22 }}
          style={{ y: m.hop }}
          stroke="#ecebe6"
          strokeWidth={1.9}
          strokeLinecap="round"
          fill="none"
        >
          <motion.path d={arc(LEFT_CX)} initial={false} animate={{ opacity: m.arcL ? 1 : 0 }} transition={{ duration: 0.15 }} />
          <motion.path d={arc(RIGHT_CX)} initial={false} animate={{ opacity: m.arcR ? 1 : 0 }} transition={{ duration: 0.15 }} />
        </motion.g>

        <motion.g
          initial={false}
          animate={m.showStars ? { opacity: 0.95, scale: [0.5, 1.15, 1], rotate: [0, 45, 90] } : { opacity: 0, scale: 0.6, rotate: 0 }}
          transition={{ duration: 0.7 }}
          style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
          stroke="#ecebe6"
          strokeWidth={1.35}
          strokeLinejoin="round"
          fill="none"
        >
          <path d={star(LEFT_CX)} />
          <path d={star(RIGHT_CX)} />
        </motion.g>

        <motion.g
          initial={false}
          animate={m.showHearts ? { opacity: 0.95, scale: [0.5, 1.14, 1] } : { opacity: 0, scale: 0.6 }}
          transition={{ duration: 0.65 }}
          style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
          stroke="#ecebe6"
          strokeWidth={1.45}
          strokeLinejoin="round"
          fill="none"
        >
          <path d={heart(LEFT_CX)} />
          <path d={heart(RIGHT_CX)} />
        </motion.g>
      </g>

      <ActivityGlyph activity={activity} color="#d6d5d0" variant="minimal" reduced={m.reduced} scale={scale} />
    </svg>
  );
}
