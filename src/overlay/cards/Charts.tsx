// SVG visualizations for explanation cards: horizontal bar chart, multi-series
// line chart, and auto-laid-out directed flow diagram. Respects reduced motion.

import { useId, useMemo } from "react";
import { motion, useReducedMotion } from "motion/react";
import { edgeLabelWidth, layoutFlow } from "./flowLayout";
import { niceScale } from "./scale";
import type { BarsBlock, FlowBlock, LineBlock } from "./types";

const NUM_FMT = new Intl.NumberFormat("ru-RU", { maximumFractionDigits: 2 });
const EASE_OUT: [number, number, number, number] = [0.22, 1, 0.36, 1];

function fmtNum(n: number): string {
  if (Math.abs(n) >= 1_000_000) return `${NUM_FMT.format(n / 1_000_000)}M`;
  if (Math.abs(n) >= 10_000) return `${NUM_FMT.format(n / 1_000)}k`;
  return NUM_FMT.format(n);
}

/** Word-wrap short text for SVG <tspan> lines. */
function wrapSvgText(text: string, maxChars: number, maxLines: number): string[] {
  const words = text.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const lines: string[] = [];
  let cur = "";

  for (const w of words) {
    const candidate = cur ? `${cur} ${w}` : w;
    if (Array.from(candidate).length <= maxChars) {
      cur = candidate;
    } else if (!cur) {
      lines.push(Array.from(w).slice(0, maxChars).join(""));
    } else {
      lines.push(cur);
      cur = w;
    }
    if (lines.length === maxLines) {
      cur = "";
      break;
    }
  }
  if (cur && lines.length < maxLines) lines.push(cur);

  // If truncated, add ellipsis to last line.
  const joined = lines.join(" ");
  if (joined.length < text.trim().length && lines.length > 0) {
    const last = lines[lines.length - 1]!;
    const chars = Array.from(last);
    lines[lines.length - 1] = `${chars.slice(0, Math.max(1, maxChars - 1)).join("").trimEnd()}…`;
  }
  return lines;
}

/** Cuts text to `maxChars` characters with an ellipsis. */
function fitText(text: string, maxChars: number): string {
  const chars = Array.from(text);
  return chars.length <= maxChars ? text : `${chars.slice(0, Math.max(1, maxChars - 1)).join("").trimEnd()}…`;
}

export function BarsChart({ block }: { block: BarsBlock }) {
  const reduced = useReducedMotion();
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const gradId = `jbar-hi-${uid}`;

  const values = block.items.map((i) => i.value);
  const minVal = Math.min(0, ...values);
  const maxVal = Math.max(0, ...values);
  const span = maxVal - minVal || 1;
  const hasExplicitHighlight = block.items.some((i) => i.highlight);

  const width = 368;
  const rowH = 34;
  const topPad = 4;
  const height = topPad + block.items.length * rowH + 4;
  const barX = 0;
  const barW = width;
  const zeroX = barX + ((0 - minVal) / span) * barW;

  return (
    <div className="jcard-chart">
      {block.title && (
        <div className="jcard-chart-head">
          <span className="jcard-chart-title">{block.title}</span>
        </div>
      )}
      <svg
        className="jcard-svg"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={block.title ? `График: ${block.title}` : "Столбчатая диаграмма"}
      >
        <defs>
          <linearGradient id={gradId} x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" stopColor="var(--amber-lo)" />
            <stop offset="0.6" stopColor="var(--amber)" />
            <stop offset="1" stopColor="var(--amber-hi)" />
          </linearGradient>
        </defs>

        {minVal < 0 && (
          <line
            x1={zeroX}
            y1={topPad}
            x2={zeroX}
            y2={height - 4}
            stroke="rgba(255,255,255,0.16)"
            strokeWidth={1}
          />
        )}

        {block.items.map((item, idx) => {
          const y = topPad + idx * rowH;
          const trackY = y + 18;
          const trackH = 8;
          const rawW = (Math.abs(item.value) / span) * barW;
          const fillW = Math.max(item.value === 0 ? 2 : 4, rawW);
          const fillX = item.value >= 0 ? zeroX : Math.max(0, zeroX - fillW);
          const isAccent = hasExplicitHighlight ? !!item.highlight : true;
          const valText = block.unit ? `${fmtNum(item.value)} ${block.unit}` : fmtNum(item.value);
          const labelChars = Math.max(6, Math.floor((width - Array.from(valText).length * 6.8 - 14) / 6.3));

          return (
            <g key={`${item.label}-${idx}`}>
              <text x={0} y={y + 12} className={`jcard-bar-label${isAccent && hasExplicitHighlight ? " is-accent" : ""}`}>
                {fitText(item.label, labelChars)}
              </text>
              <text x={width} y={y + 12} textAnchor="end" className={`jcard-bar-val${isAccent && hasExplicitHighlight ? " is-accent" : ""}`}>
                {valText}
              </text>
              <rect x={barX} y={trackY} width={barW} height={trackH} rx={4} className="jcard-bar-track" />
              <motion.rect
                y={trackY}
                height={trackH}
                rx={4}
                fill={isAccent ? `url(#${gradId})` : "rgba(236, 235, 230, 0.36)"}
                initial={reduced ? { width: fillW, x: fillX } : { width: 0, x: zeroX }}
                animate={{ width: fillW, x: fillX }}
                transition={reduced ? { duration: 0 } : { duration: 0.45, delay: idx * 0.045, ease: EASE_OUT }}
              >
                <title>{`${item.label}: ${valText}`}</title>
              </motion.rect>
            </g>
          );
        })}
      </svg>
    </div>
  );
}

const LINE_COLORS = ["var(--amber)", "var(--bone)", "var(--ok)", "var(--text-3)"] as const;

export function LineChart({ block }: { block: LineBlock }) {
  const reduced = useReducedMotion();
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const areaGrad = `jline-area-${uid}`;

  const allVals = block.series.flatMap((s) => s.values);
  const scale = niceScale(Math.min(...allVals), Math.max(...allVals));
  const minY = scale.min;
  const spanY = scale.max - scale.min || 1;

  const W = 368;
  const H = 152;
  const padL = 38;
  const padR = 12;
  const padT = 12;
  const padB = 24;
  const plotW = W - padL - padR;
  const plotH = H - padT - padB;
  const nX = Math.max(2, block.xLabels.length);

  const xAt = (i: number) => padL + (i / (nX - 1)) * plotW;
  const yAt = (v: number) => padT + (1 - (v - minY) / spanY) * plotH;

  const ticks = scale.ticks.map((v) => ({ y: yAt(v), label: fmtNum(v) }));

  const labelStep = nX > 8 ? Math.ceil(nX / 6) : 1;
  const shownLabels = Math.ceil(nX / labelStep);
  const xLabelChars = Math.max(3, Math.floor(plotW / shownLabels / 6.2));
  const showLegend = block.series.length > 1 || block.series.some((s) => !!s.name);

  return (
    <div className="jcard-chart">
      {(block.title || block.unit) && (
        <div className="jcard-chart-head">
          {block.title && <span className="jcard-chart-title">{block.title}</span>}
          {block.unit && <span className="jcard-chart-unit">{block.unit}</span>}
        </div>
      )}

      {showLegend && (
        <div className="jcard-legend">
          {block.series.map((s, si) => (
            <span key={si} className="jcard-legend-item">
              <i style={{ background: LINE_COLORS[si % LINE_COLORS.length] }} />
              {s.name ?? `Ряд ${si + 1}`}
            </span>
          ))}
        </div>
      )}

      <svg
        className="jcard-svg"
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={block.title ? `Линейный график: ${block.title}` : "Линейный график"}
      >
        <defs>
          <linearGradient id={areaGrad} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--amber)" stopOpacity={0.26} />
            <stop offset="1" stopColor="var(--amber)" stopOpacity={0.01} />
          </linearGradient>
        </defs>

        {ticks.map((tk, i) => (
          <g key={i}>
            <line
              x1={padL}
              y1={tk.y}
              x2={W - padR}
              y2={tk.y}
              stroke="rgba(255,255,255,0.08)"
              strokeDasharray={i === 0 ? undefined : "2 3"}
              strokeWidth={1}
            />
            <text x={padL - 6} y={tk.y + 3.5} textAnchor="end" className="jcard-axis-text">
              {tk.label}
            </text>
          </g>
        ))}

        {block.xLabels.map((lbl, i) => {
          if (i !== 0 && i !== nX - 1 && i % labelStep !== 0) return null;
          const anchor = i === 0 ? "start" : i === nX - 1 ? "end" : "middle";
          return (
            <text key={i} x={xAt(i)} y={H - 6} textAnchor={anchor} className="jcard-axis-text">
              {fitText(lbl, xLabelChars)}
            </text>
          );
        })}

        {block.series.map((s, si) => {
          const color = LINE_COLORS[si % LINE_COLORS.length]!;
          const pts = s.values.map((v, i) => ({ x: xAt(i), y: yAt(v), v, lbl: block.xLabels[i] ?? String(i + 1) }));
          const linePath = pts.map((p, i) => `${i === 0 ? "M" : "L"} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(" ");
          const areaPath =
            si === 0 && block.series.length === 1 && pts.length >= 2
              ? `${linePath} L ${pts[pts.length - 1]!.x.toFixed(1)} ${(padT + plotH).toFixed(1)} L ${pts[0]!.x.toFixed(1)} ${(padT + plotH).toFixed(1)} Z`
              : null;

          return (
            <g key={si}>
              {areaPath && (
                <motion.path
                  d={areaPath}
                  fill={`url(#${areaGrad})`}
                  initial={reduced ? { opacity: 1 } : { opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={reduced ? { duration: 0 } : { duration: 0.45, delay: 0.12 }}
                />
              )}
              <motion.path
                d={linePath}
                fill="none"
                stroke={color}
                strokeWidth={si === 0 ? 2.1 : 1.7}
                strokeLinecap="round"
                strokeLinejoin="round"
                initial={reduced ? { pathLength: 1 } : { pathLength: 0 }}
                animate={{ pathLength: 1 }}
                transition={reduced ? { duration: 0 } : { duration: 0.55, delay: si * 0.08, ease: EASE_OUT }}
              />
              {pts.map((p, pi) => (
                <motion.circle
                  key={pi}
                  cx={p.x}
                  cy={p.y}
                  r={si === 0 ? 3.2 : 2.6}
                  fill="#0b0b0e"
                  stroke={color}
                  strokeWidth={1.8}
                  initial={reduced ? { opacity: 1 } : { opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={reduced ? { duration: 0 } : { duration: 0.2, delay: 0.28 + pi * 0.025 }}
                >
                  <title>{`${s.name ? `${s.name} · ` : ""}${p.lbl}: ${fmtNum(p.v)}${block.unit ? ` ${block.unit}` : ""}`}</title>
                </motion.circle>
              ))}
            </g>
          );
        })}
      </svg>
    </div>
  );
}

export function FlowDiagram({ block }: { block: FlowBlock }) {
  const reduced = useReducedMotion();
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const arrowId = `jflow-arr-${uid}`;
  const layout = useMemo(() => layoutFlow(block), [block]);

  if (layout.nodes.length === 0) return null;

  return (
    <div className="jcard-flow">
      <svg
        className="jcard-svg jcard-flow-svg"
        viewBox={`0 0 ${layout.width} ${layout.height}`}
        style={{ width: layout.width, maxWidth: "100%" }}
        role="img"
        aria-label="Схема процесса"
      >
        <defs>
          <marker
            id={arrowId}
            viewBox="0 0 10 10"
            refX="8.5"
            refY="5"
            markerWidth="6"
            markerHeight="6"
            orient="auto-start-reverse"
          >
            <path d="M 1 1.5 L 8.5 5 L 1 8.5 z" fill="rgba(255, 178, 63, 0.72)" />
          </marker>
        </defs>

        {layout.edges.map((edge, idx) => (
          <g key={`${edge.from}-${edge.to}-${idx}`}>
            <motion.path
              d={edge.path}
              fill="none"
              stroke={edge.backEdge ? "rgba(255, 178, 63, 0.35)" : "rgba(255, 255, 255, 0.28)"}
              strokeWidth={1.35}
              strokeDasharray={edge.backEdge ? "4 3" : undefined}
              markerEnd={`url(#${arrowId})`}
              initial={reduced ? { opacity: 1 } : { opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={reduced ? { duration: 0 } : { duration: 0.3, delay: 0.12 + idx * 0.03 }}
            />
            {edge.label && edge.labelPos && (
              <g transform={`translate(${edge.labelPos.x}, ${edge.labelPos.y})`}>
                {(() => {
                  const pillW = edgeLabelWidth(edge.label);
                  return (
                    <>
                      <rect
                        x={-pillW / 2}
                        y={-9}
                        width={pillW}
                        height={17}
                        rx={8.5}
                        fill="#101014"
                        stroke="rgba(255,255,255,0.12)"
                        strokeWidth={0.8}
                      />
                      <text textAnchor="middle" y={3} className="jcard-flow-edge-label">
                        {edge.label}
                      </text>
                    </>
                  );
                })()}
              </g>
            )}
          </g>
        ))}

        {layout.nodes.map((node, idx) => {
          const maxChars = Math.max(8, Math.floor((node.w - 18) / 6.7));
          const labelLines = wrapSvgText(node.label, maxChars, 3);
          const noteLines = node.note ? wrapSvgText(node.note, Math.max(10, Math.floor((node.w - 18) / 5.8)), 2) : [];
          const totalTextH = labelLines.length * 14 + (noteLines.length > 0 ? 4 + noteLines.length * 12 : 0);
          const startY = node.y + (node.h - totalTextH) / 2 + 11;

          // Opaque fills: an edge passing behind a box never shows through its text.
          const fill = node.tone === "accent" ? "#2a2015" : node.tone === "muted" ? "#121215" : "#1a1a1f";
          const stroke =
            node.tone === "accent"
              ? "rgba(255, 178, 63, 0.52)"
              : node.tone === "muted"
                ? "rgba(255, 255, 255, 0.09)"
                : "rgba(255, 255, 255, 0.16)";

          return (
            <motion.g
              key={node.id}
              initial={reduced ? { opacity: 1 } : { opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              transition={reduced ? { duration: 0 } : { duration: 0.26, delay: idx * 0.04, ease: EASE_OUT }}
            >
              <rect
                x={node.x}
                y={node.y}
                width={node.w}
                height={node.h}
                rx={9}
                fill={fill}
                stroke={stroke}
                strokeWidth={node.tone === "accent" ? 1.2 : 1}
              />
              <text
                x={node.x + node.w / 2}
                y={startY}
                textAnchor="middle"
                className={`jcard-flow-label is-${node.tone}`}
              >
                {labelLines.map((line, li) => (
                  <tspan key={li} x={node.x + node.w / 2} dy={li === 0 ? 0 : 14}>
                    {line}
                  </tspan>
                ))}
              </text>
              {noteLines.length > 0 && (
                <text
                  x={node.x + node.w / 2}
                  y={startY + labelLines.length * 14 + 2}
                  textAnchor="middle"
                  className="jcard-flow-note"
                >
                  {noteLines.map((line, li) => (
                    <tspan key={li} x={node.x + node.w / 2} dy={li === 0 ? 0 : 12}>
                      {line}
                    </tspan>
                  ))}
                </text>
              )}
            </motion.g>
          );
        })}
      </svg>
    </div>
  );
}
