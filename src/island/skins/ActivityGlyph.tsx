// Small, tasteful micro-glyph next to JARVIS's eyes while a tool is running.
// Drawn inside a 9x9 local box placed cleanly to the upper-right of the eyes
// (within the reserved 72px brow slot) and adapted to each skin's style.

import { motion } from "motion/react";
import type { Activity } from "../../store/emote";

export interface ActivityGlyphProps {
  activity: Activity | null;
  color: string;
  variant?: "soft" | "pixel" | "minimal";
  reduced?: boolean;
  /** Hide on tiny scaled lip mode so it never clutters. */
  scale?: number;
}

function PixelGlyph({ activity, color }: { activity: Activity; color: string }) {
  // Crisp 1.5-unit pixel blocks on a 9x9 grid.
  const b = (x: number, y: number, w = 1.5, h = 1.5, key?: string) => (
    <rect key={key ?? `${x}-${y}`} x={x} y={y} width={w} height={h} fill={color} shapeRendering="crispEdges" />
  );
  switch (activity) {
    case "search":
      return (
        <>
          {b(2, 1, 3, 1.5)}
          {b(1, 2.5, 1.5, 2.5)}
          {b(5, 2.5, 1.5, 2.5)}
          {b(2, 5, 3, 1.5)}
          {b(6, 6, 1.5, 1.5)}
          {b(7.2, 7.2, 1.5, 1.5)}
        </>
      );
    case "read":
      return (
        <>
          {b(1, 1.5, 7, 1.4)}
          {b(1, 4, 5.5, 1.4)}
          {b(1, 6.5, 6.5, 1.4)}
        </>
      );
    case "type":
      return (
        <>
          {b(1.5, 6.2, 6, 1.6)}
          {b(2, 2.2, 1.5, 1.5)}
          {b(5.5, 2.2, 1.5, 1.5)}
        </>
      );
    case "click":
      return (
        <>
          {b(3.5, 3.5, 2, 2)}
          {b(3.8, 0.8, 1.4, 1.4)}
          {b(3.8, 6.8, 1.4, 1.4)}
          {b(0.8, 3.8, 1.4, 1.4)}
          {b(6.8, 3.8, 1.4, 1.4)}
        </>
      );
    case "look":
      return (
        <>
          {b(0.8, 1.2, 2.4, 1.3)}
          {b(0.8, 1.2, 1.3, 2.4)}
          {b(5.8, 1.2, 2.4, 1.3)}
          {b(6.9, 1.2, 1.3, 2.4)}
          {b(0.8, 6.5, 2.4, 1.3)}
          {b(0.8, 5.4, 1.3, 2.4)}
          {b(5.8, 6.5, 2.4, 1.3)}
          {b(6.9, 5.4, 1.3, 2.4)}
        </>
      );
    case "draw":
      return (
        <>
          {b(1.2, 6.2, 1.6, 1.6)}
          {b(3, 4.4, 1.6, 1.6)}
          {b(4.8, 2.6, 1.6, 1.6)}
          {b(6.4, 1.2, 1.6, 1.6)}
        </>
      );
    case "files":
      return (
        <>
          {b(1, 1.5, 3.2, 1.4)}
          {b(1, 3, 7, 4.5)}
        </>
      );
    case "music":
      return (
        <>
          {b(3.5, 1.5, 4, 1.4)}
          {b(3.5, 1.5, 1.4, 5)}
          {b(6.1, 1.5, 1.4, 4.5)}
          {b(1.8, 5.5, 2.4, 2)}
          {b(5, 5, 2.4, 2)}
        </>
      );
    case "calendar":
      return (
        <>
          {b(1, 1.2, 7, 1.6)}
          {b(1.5, 4, 1.6, 1.6)}
          {b(4, 4, 1.6, 1.6)}
          {b(6.2, 4, 1.6, 1.6)}
          {b(1.5, 6.3, 1.6, 1.6)}
          {b(4, 6.3, 1.6, 1.6)}
        </>
      );
    case "memory":
      return (
        <>
          {b(3.7, 0.8, 1.6, 2.2)}
          {b(3.7, 6, 1.6, 2.2)}
          {b(0.8, 3.7, 2.2, 1.6)}
          {b(6, 3.7, 2.2, 1.6)}
          {b(3.7, 3.7, 1.6, 1.6)}
        </>
      );
    case "time":
      return (
        <>
          {b(2.2, 0.8, 4.6, 1.3)}
          {b(2.2, 6.9, 4.6, 1.3)}
          {b(0.8, 2.2, 1.3, 4.6)}
          {b(6.9, 2.2, 1.3, 4.6)}
          {b(3.9, 2.6, 1.3, 2.6)}
          {b(3.9, 4.4, 2.3, 1.3)}
        </>
      );
    case "system":
      return (
        <>
          {b(1, 2.2, 7, 1.4)}
          {b(5.2, 1.4, 1.6, 3)}
          {b(1, 5.8, 7, 1.4)}
          {b(2.4, 5, 1.6, 3)}
        </>
      );
    case "code":
      return (
        <>
          {b(1.2, 1.8, 1.5, 1.5)}
          {b(2.7, 3.3, 1.5, 1.5)}
          {b(1.2, 4.8, 1.5, 1.5)}
          {b(4.6, 5.4, 3.4, 1.5)}
        </>
      );
    case "connect":
      return (
        <>
          {b(1, 3.6, 2, 2)}
          {b(6, 3.6, 2, 2)}
          {b(3.6, 4, 1.8, 1.2)}
        </>
      );
    case "speak":
      return (
        <>
          {b(1.4, 3.6, 1.5, 2)}
          {b(3.8, 2.4, 1.5, 4.4)}
          {b(6.2, 1.2, 1.5, 6.8)}
        </>
      );
    case "work":
      return (
        <>
          {b(1.2, 3.7, 1.6, 1.6)}
          {b(3.7, 3.7, 1.6, 1.6)}
          {b(6.2, 3.7, 1.6, 1.6)}
        </>
      );
  }
}

function SmoothGlyph({ activity, color, strokeW }: { activity: Activity; color: string; strokeW: number }) {
  const common = {
    stroke: color,
    strokeWidth: strokeW,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    fill: "none",
  };
  switch (activity) {
    case "search":
      return (
        <>
          <circle cx={3.8} cy={3.8} r={2.3} {...common} />
          <path d="M5.6 5.6 L7.8 7.8" {...common} />
        </>
      );
    case "read":
      return (
        <>
          <path d="M1.4 2.2 H7.6 M1.4 4.6 H6.2 M1.4 7 H7" {...common} />
        </>
      );
    case "type":
      return (
        <>
          <path d="M1.6 6.8 H7.4" {...common} />
          <path d="M3 2.4 V4.8 M6 2.4 V4.8" {...common} />
        </>
      );
    case "click":
      return (
        <>
          <circle cx={4.5} cy={4.5} r={1.3} fill={color} />
          <circle cx={4.5} cy={4.5} r={3.3} {...common} strokeDasharray="2 2.2" />
        </>
      );
    case "look":
      return (
        <path
          d="M1.2 3.2 V1.6 H2.8 M6.2 1.6 H7.8 V3.2 M1.2 5.8 V7.4 H2.8 M6.2 7.4 H7.8 V5.8"
          {...common}
        />
      );
    case "draw":
      return <path d="M1.4 7.2 Q3.2 2.2 7.6 1.8" {...common} />;
    case "files":
      return <path d="M1.4 2.4 H3.7 L4.6 3.5 H7.6 V7.1 H1.4 Z" {...common} />;
    case "music":
      return (
        <>
          <path d="M3.4 6.6 V2.3 L7.4 1.6 V5.8" {...common} />
          <circle cx={2.4} cy={6.8} r={1.1} fill={color} />
          <circle cx={6.4} cy={6.1} r={1.1} fill={color} />
        </>
      );
    case "calendar":
      return (
        <>
          <rect x={1.4} y={2} width={6.2} height={5.6} rx={1.1} {...common} />
          <path d="M1.4 3.9 H7.6 M3.2 1.1 V2.4 M5.8 1.1 V2.4" {...common} />
        </>
      );
    case "memory":
      return (
        <path
          d="M4.5 1.1 Q4.8 4.1 7.9 4.5 Q4.8 4.9 4.5 7.9 Q4.2 4.9 1.1 4.5 Q4.2 4.1 4.5 1.1 Z"
          fill={color}
        />
      );
    case "time":
      return (
        <>
          <circle cx={4.5} cy={4.5} r={3.2} {...common} />
          <path d="M4.5 2.7 V4.6 L6 5.5" {...common} />
        </>
      );
    case "system":
      return (
        <>
          <path d="M1.4 3 H7.6 M1.4 6 H7.6" {...common} />
          <circle cx={5.6} cy={3} r={1} fill={color} />
          <circle cx={3.2} cy={6} r={1} fill={color} />
        </>
      );
    case "code":
      return <path d="M1.4 2.4 L3.9 4.5 L1.4 6.6 M4.8 6.6 H7.8" {...common} />;
    case "connect":
      return (
        <>
          <circle cx={2.2} cy={4.5} r={1.2} fill={color} />
          <circle cx={6.8} cy={4.5} r={1.2} fill={color} />
          <path d="M3.7 4.5 H5.3" {...common} />
        </>
      );
    case "speak":
      return <path d="M2.2 3.2 Q3.5 4.5 2.2 5.8 M4.5 2.1 Q6.3 4.5 4.5 6.9 M6.8 1.3 Q8.8 4.5 6.8 7.7" {...common} />;
    case "work":
      return (
        <>
          <circle cx={2} cy={4.5} r={0.95} fill={color} />
          <circle cx={4.5} cy={4.5} r={0.95} fill={color} />
          <circle cx={7} cy={4.5} r={0.95} fill={color} />
        </>
      );
  }
}

export function ActivityGlyph({ activity, color, variant = "soft", reduced = false, scale = 1 }: ActivityGlyphProps) {
  if (!activity || scale < 0.6) return null;
  const strokeW = variant === "minimal" ? 0.85 : 1.05;
  // The placement lives on a plain group: motion writes its own transform on
  // the animated one, which would drop an SVG `transform` attribute.
  return (
    <g transform="translate(41.5, 2.5)">
      <motion.g
        key={activity}
        initial={{ opacity: 0, scale: 0.7 }}
        animate={reduced ? { opacity: 0.82, scale: 1 } : { opacity: [0.65, 0.98, 0.65], scale: 1 }}
        transition={reduced ? { duration: 0.12 } : { opacity: { duration: 1.6, repeat: Infinity, ease: "easeInOut" }, scale: { duration: 0.2 } }}
        style={{ transformOrigin: "50% 50%", transformBox: "fill-box" }}
      >
        {variant === "pixel" ? <PixelGlyph activity={activity} color={color} /> : <SmoothGlyph activity={activity} color={color} strokeW={strokeW} />}
      </motion.g>
    </g>
  );
}
