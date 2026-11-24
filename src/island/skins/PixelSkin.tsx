// «Пиксель» — crisp chunky pixel-art eyes built strictly from SVG rects on a
// 2px integer grid, with stepped animations and authentic pixel-art emotes.

import { motion, useTransform, type MotionValue } from "motion/react";
import { useId } from "react";
import { ActivityGlyph } from "./ActivityGlyph";
import type { EyesProps } from "./types";
import { BOX_H, BOX_W, LEFT_CX, RIGHT_CX, useFaceMotion } from "./useFaceMotion";

const PX = 2; // grid cell size in viewBox units
const PIXEL_W = 6; // 3 cells wide
const snap = (v: number, step = PX) => Math.round(v / step) * step;

// 5x5 pixel-art bitmaps (1 = lit cell, rendered at 1.8px per sub-pixel)
const HEART_MAP = [
  "01010",
  "11111",
  "11111",
  "01110",
  "00100",
];

const STAR_MAP = [
  "00100",
  "01110",
  "11111",
  "01110",
  "00100",
];

const ARC_MAP = [
  "01110",
  "10001",
  "10001",
];

function PixelBitmap({
  cx,
  cy,
  rows,
  cell,
  fill,
  hi,
}: {
  cx: number;
  cy: number;
  rows: string[];
  cell: number;
  fill: string;
  hi: string;
}) {
  const w = rows[0]!.length * cell;
  const h = rows.length * cell;
  const x0 = Math.round(cx - w / 2);
  const y0 = Math.round(cy - h / 2);
  const cells: { x: number; y: number; top: boolean }[] = [];
  rows.forEach((row, r) => {
    for (let c = 0; c < row.length; c++) {
      if (row[c] === "1") cells.push({ x: x0 + c * cell, y: y0 + r * cell, top: r === 0 });
    }
  });
  return (
    <>
      {cells.map((p, i) => (
        <rect key={i} x={p.x} y={p.y} width={cell} height={cell} fill={p.top ? hi : fill} shapeRendering="crispEdges" />
      ))}
    </>
  );
}

/** Top and height of one of the eye's three columns once the lid cuts it;
 *  a tilted lid cuts the columns one step apart. The outer columns are one
 *  cell shorter at both ends, so a tall eye reads as a pixel-art capsule. */
function usePixelColumn(
  sy: MotionValue<number>,
  sh: MotionValue<number>,
  cutBase: MotionValue<number>,
  tiltStep: MotionValue<number>,
  col: number,
): [MotionValue<number>, MotionValue<number>] {
  const cut = () => Math.max(0, cutBase.get() + (col - 1) * tiltStep.get());
  const inset = () => (col !== 1 && sh.get() >= PX * 4 ? PX : 0);
  const y = useTransform(() => sy.get() + Math.min(sh.get() - PX, Math.max(cut(), inset())));
  const h = useTransform(() => Math.max(PX, sh.get() - Math.max(cut(), inset()) - inset()));
  return [y, h];
}

function PixelEye({
  x,
  y,
  h,
  lid,
  tilt,
  error,
}: {
  x: MotionValue<number>;
  y: MotionValue<number>;
  h: MotionValue<number>;
  lid: MotionValue<number>;
  tilt: MotionValue<number>;
  error: boolean;
}) {
  // Quantize to discrete 2px steps for authentic stepped retro animation.
  const sx = useTransform(() => snap(x.get(), 1));
  const sh = useTransform(() => Math.max(PX, snap(h.get(), PX)));
  const sy = useTransform(() => snap(y.get() + (h.get() - sh.get()) / 2, 1));

  // Lid mask cut in stepped blocks across the 3 columns.
  const cutBase = useTransform(() => snap(lid.get() * sh.get() * 0.52, PX));
  const tiltStep = useTransform(() => (Math.abs(tilt.get()) > 7 ? Math.sign(tilt.get()) * PX : 0));

  const c0x = sx;
  const c1x = useTransform(() => sx.get() + PX);
  const c2x = useTransform(() => sx.get() + PX * 2);
  const [c0y, c0h] = usePixelColumn(sy, sh, cutBase, tiltStep, 0);
  const [c1y, c1h] = usePixelColumn(sy, sh, cutBase, tiltStep, 1);
  const [c2y, c2h] = usePixelColumn(sy, sh, cutBase, tiltStep, 2);

  const botY = useTransform(() => c1y.get() + c1h.get() - PX);
  const showGlint = useTransform(() => (sh.get() >= 6 && cutBase.get() < 4 ? 1 : 0));
  const glintY = useTransform(() => c0y.get());

  const mainFill = error ? "var(--danger)" : "#6ff0a6";
  const shadeFill = error ? "#c94336" : "#2bb56e";
  const hiFill = error ? "#ffd2ca" : "#d8ffea";

  return (
    <>
      <motion.rect x={c0x} y={c0y} width={PX} height={c0h} fill={mainFill} shapeRendering="crispEdges" />
      <motion.rect x={c1x} y={c1y} width={PX} height={c1h} fill={mainFill} shapeRendering="crispEdges" />
      <motion.rect x={c2x} y={c2y} width={PX} height={c2h} fill={mainFill} shapeRendering="crispEdges" />
      {/* Shade: the bottom cell of the middle column and the right column. */}
      <motion.rect x={c1x} y={botY} width={PX} height={PX} fill={shadeFill} opacity={0.55} shapeRendering="crispEdges" />
      <motion.rect x={c2x} y={c2y} width={PX} height={c2h} fill={shadeFill} opacity={0.35} shapeRendering="crispEdges" />
      {/* Crisp 2x2 pixel glint in upper-left cell */}
      <motion.rect x={c0x} y={glintY} width={PX} height={PX} fill={hiFill} opacity={showGlint} shapeRendering="crispEdges" />
    </>
  );
}

export function PixelSkin(props: EyesProps) {
  const { mood, scale = 1, activity = null, onBoop } = props;
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const glowId = `px-glow-${uid}`;

  const m = useFaceMotion(props);
  const hopSnap = useTransform(() => snap(m.hop.get(), 1));
  const splitLeftX = useTransform(() => snap(m.leftX.get(), 1) - PX);
  const splitRightX = useTransform(() => snap(m.rightX.get(), 1) + PX);
  const splitY = useTransform(() => snap(m.yL.get(), 1));
  const splitH = useTransform(() => Math.max(PX, snap(m.hL.get(), PX)));

  const split = mood === "thinking";
  const error = mood === "error";

  return (
    <svg
      width={BOX_W * scale}
      height={BOX_H * scale}
      viewBox={`0 0 ${BOX_W} ${BOX_H}`}
      style={{ overflow: "visible", display: "block", imageRendering: "pixelated", pointerEvents: onBoop ? "auto" : undefined }}
      aria-hidden="true"
      onDoubleClick={onBoop}
    >
      <defs>
        <filter id={glowId} x="-150%" y="-150%" width="400%" height="400%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="2.2" result="halo" />
          <feComponentTransfer in="halo" result="soft">
            <feFuncA type="linear" slope="0.34" />
          </feComponentTransfer>
          <feMerge>
            <feMergeNode in="soft" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {split && (
        <g opacity={0.42}>
          <motion.rect x={splitLeftX} y={splitY} width={PIXEL_W} height={splitH} fill="#62d6ff" shapeRendering="crispEdges" />
          <motion.rect x={splitRightX} y={splitY} width={PIXEL_W} height={splitH} fill="#ff6b8b" shapeRendering="crispEdges" />
        </g>
      )}

      <g filter={`url(#${glowId})`}>
        <motion.g initial={false} animate={{ opacity: m.pillsHidden ? 0 : mood === "sleep" ? 0.45 : 1 }} transition={{ duration: 0.12 }}>
          <motion.g style={{ opacity: m.life }}>
            <motion.g initial={false} animate={{ opacity: m.arcL ? 0 : 1 }} transition={{ duration: 0.08 }}>
              <PixelEye x={m.leftX} y={m.yL} h={m.hL} lid={m.lidL} tilt={m.tiltL} error={error} />
            </motion.g>
            <motion.g initial={false} animate={{ opacity: m.arcR ? 0 : 1 }} transition={{ duration: 0.08 }}>
              <PixelEye x={m.rightX} y={m.yR} h={m.hR} lid={m.lidR} tilt={m.tiltR} error={error} />
            </motion.g>
          </motion.g>
        </motion.g>

        {/* Pixel smiling arcs (^ ^) */}
        <motion.g
          initial={false}
          animate={{ opacity: m.arcL || m.arcR ? 1 : 0 }}
          transition={{ duration: 0.1 }}
          style={{ y: hopSnap }}
        >
          <motion.g initial={false} animate={{ opacity: m.arcL ? 1 : 0 }} transition={{ duration: 0.08 }}>
            <PixelBitmap cx={LEFT_CX} cy={BOX_H / 2} rows={ARC_MAP} cell={1.8} fill="#6ff0a6" hi="#d8ffea" />
          </motion.g>
          <motion.g initial={false} animate={{ opacity: m.arcR ? 1 : 0 }} transition={{ duration: 0.08 }}>
            <PixelBitmap cx={RIGHT_CX} cy={BOX_H / 2} rows={ARC_MAP} cell={1.8} fill="#6ff0a6" hi="#d8ffea" />
          </motion.g>
        </motion.g>

        {/* Pixel hearts */}
        <motion.g
          initial={false}
          animate={m.showHearts ? { opacity: 1, scale: [0.6, 1.15, 1] } : { opacity: 0, scale: 0.6 }}
          transition={{ duration: 0.35 }}
          style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        >
          <PixelBitmap cx={LEFT_CX} cy={BOX_H / 2} rows={HEART_MAP} cell={1.8} fill="#6ff0a6" hi="#d8ffea" />
          <PixelBitmap cx={RIGHT_CX} cy={BOX_H / 2} rows={HEART_MAP} cell={1.8} fill="#6ff0a6" hi="#d8ffea" />
        </motion.g>

        {/* Pixel stars */}
        <motion.g
          initial={false}
          animate={m.showStars ? { opacity: 1, scale: [0.6, 1.2, 1] } : { opacity: 0, scale: 0.6 }}
          transition={{ duration: 0.35 }}
          style={{ transformOrigin: `${BOX_W / 2}px ${BOX_H / 2}px`, transformBox: "view-box" }}
        >
          <PixelBitmap cx={LEFT_CX} cy={BOX_H / 2} rows={STAR_MAP} cell={1.8} fill="#6ff0a6" hi="#d8ffea" />
          <PixelBitmap cx={RIGHT_CX} cy={BOX_H / 2} rows={STAR_MAP} cell={1.8} fill="#6ff0a6" hi="#d8ffea" />
        </motion.g>
      </g>

      <ActivityGlyph activity={activity} color="#a8ffd0" variant="pixel" reduced={m.reduced} scale={scale} />
    </svg>
  );
}
