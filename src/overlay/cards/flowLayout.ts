// Pure layered DAG auto-layout for `flow` explanation blocks.
// Handles left->right ("lr") and top->down ("td") directions, breaks cycles
// gracefully, reduces edge crossings via barycenter sweeps, guarantees zero
// node overlaps, and outputs ready-to-render SVG paths and label positions.

import type { FlowBlock, FlowDirection, FlowTone } from "./types";

export interface FlowNodeBox {
  id: string;
  label: string;
  note?: string;
  tone: FlowTone;
  layer: number;
  order: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface FlowEdgeRoute {
  from: string;
  to: string;
  label?: string;
  path: string;
  labelPos?: { x: number; y: number };
  backEdge: boolean;
}

export interface FlowLayout {
  direction: FlowDirection;
  width: number;
  height: number;
  nodes: FlowNodeBox[];
  edges: FlowEdgeRoute[];
}

const PAD = 14;
const SIBLING_GAP_TD = 14;
const SIBLING_GAP_LR = 12;
const LAYER_GAP_TD = 34;
const LAYER_GAP_LR = 40;
const LABEL_GAP_BONUS = 14;

/** Approximate box dimensions from label and optional sub-note length. */
function measureNode(label: string, note: string | undefined, direction: FlowDirection): { w: number; h: number } {
  const maxW = direction === "lr" ? 132 : 176;
  const minW = direction === "lr" ? 84 : 104;
  const charW = 6.7;
  const noteCharW = 5.8;
  const padX = 20;

  const labelLen = Array.from(label).length;
  const noteLen = note ? Array.from(note).length : 0;
  const desiredW = Math.max(labelLen * charW, noteLen * noteCharW) + padX;
  const w = Math.round(Math.max(minW, Math.min(maxW, desiredW)));

  const usableChars = Math.max(8, Math.floor((w - padX) / charW));
  const labelLines = Math.min(3, Math.max(1, Math.ceil(labelLen / usableChars)));
  const noteLines = note ? Math.min(2, Math.max(1, Math.ceil(noteLen / Math.max(10, Math.floor((w - padX) / noteCharW))))) : 0;
  const h = Math.round(14 + labelLines * 15 + (noteLines > 0 ? 4 + noteLines * 13 : 0));

  return { w, h };
}

/** Detect edges that close cycles via DFS so layering always runs on a DAG. */
function findBackEdges(nodeIds: string[], edges: { from: string; to: string }[]): Set<number> {
  const adj = new Map<string, { to: string; idx: number }[]>();
  for (const id of nodeIds) adj.set(id, []);
  edges.forEach((e, idx) => {
    adj.get(e.from)?.push({ to: e.to, idx });
  });

  const state = new Map<string, 0 | 1 | 2>(); // 0 unvisited, 1 active, 2 done
  const back = new Set<number>();

  const dfs = (u: string) => {
    state.set(u, 1);
    for (const { to, idx } of adj.get(u) ?? []) {
      const s = state.get(to) ?? 0;
      if (s === 1) {
        back.add(idx);
      } else if (s === 0) {
        dfs(to);
      }
    }
    state.set(u, 2);
  };

  for (const id of nodeIds) {
    if ((state.get(id) ?? 0) === 0) dfs(id);
  }
  return back;
}

/** Longest-path layering on the acyclic edge subset. */
function assignLayers(nodeIds: string[], edges: { from: string; to: string }[], backEdges: Set<number>): Map<string, number> {
  const indeg = new Map<string, number>();
  const out = new Map<string, string[]>();
  for (const id of nodeIds) {
    indeg.set(id, 0);
    out.set(id, []);
  }
  edges.forEach((e, idx) => {
    if (backEdges.has(idx) || e.from === e.to) return;
    out.get(e.from)?.push(e.to);
    indeg.set(e.to, (indeg.get(e.to) ?? 0) + 1);
  });

  const layer = new Map<string, number>();
  const queue: string[] = [];
  for (const id of nodeIds) {
    if ((indeg.get(id) ?? 0) === 0) {
      queue.push(id);
      layer.set(id, 0);
    }
  }

  let head = 0;
  while (head < queue.length) {
    const u = queue[head++]!;
    const lu = layer.get(u) ?? 0;
    for (const v of out.get(u) ?? []) {
      const nextL = lu + 1;
      if ((layer.get(v) ?? 0) < nextL) layer.set(v, nextL);
      const rem = (indeg.get(v) ?? 1) - 1;
      indeg.set(v, rem);
      if (rem === 0) queue.push(v);
    }
  }

  for (const id of nodeIds) {
    if (!layer.has(id)) layer.set(id, 0);
  }
  return layer;
}

function round1(n: number): number {
  return Math.round(n * 10) / 10;
}

function cubicPoint(
  t: number,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  x3: number,
  y3: number,
): { x: number; y: number } {
  const mt = 1 - t;
  const x = mt * mt * mt * x0 + 3 * mt * mt * t * x1 + 3 * mt * t * t * x2 + t * t * t * x3;
  const y = mt * mt * mt * y0 + 3 * mt * mt * t * y1 + 3 * mt * t * t * y2 + t * t * t * y3;
  return { x: round1(x), y: round1(y) };
}

export function layoutFlow(block: FlowBlock): FlowLayout {
  const nodesInput = block.nodes;
  if (nodesInput.length === 0) {
    return { direction: block.direction ?? "td", width: 0, height: 0, nodes: [], edges: [] };
  }

  const indexOf = new Map<string, number>();
  const nodeIds: string[] = [];
  nodesInput.forEach((n, idx) => {
    if (!indexOf.has(n.id)) {
      indexOf.set(n.id, idx);
      nodeIds.push(n.id);
    }
  });

  const validEdges = block.edges.filter((e) => indexOf.has(e.from) && indexOf.has(e.to));
  const backEdges = findBackEdges(nodeIds, validEdges);
  const layerMap = assignLayers(nodeIds, validEdges, backEdges);

  const maxLayer = Math.max(0, ...Array.from(layerMap.values()));
  const layers: string[][] = Array.from({ length: maxLayer + 1 }, () => []);
  for (const id of nodeIds) {
    layers[layerMap.get(id) ?? 0]!.push(id);
  }

  // Two-pass barycenter crossing reduction.
  const parentsOf = new Map<string, string[]>();
  const childrenOf = new Map<string, string[]>();
  for (const id of nodeIds) {
    parentsOf.set(id, []);
    childrenOf.set(id, []);
  }
  validEdges.forEach((e, idx) => {
    if (backEdges.has(idx) || e.from === e.to) return;
    parentsOf.get(e.to)!.push(e.from);
    childrenOf.get(e.from)!.push(e.to);
  });

  const orderPos = new Map<string, number>();
  const refreshPos = () => {
    layers.forEach((arr) => arr.forEach((id, i) => orderPos.set(id, i)));
  };
  refreshPos();

  const sortByNeighbors = (arr: string[], neighbors: Map<string, string[]>) => {
    arr.sort((a, b) => {
      const na = neighbors.get(a) ?? [];
      const nb = neighbors.get(b) ?? [];
      const avgA = na.length ? na.reduce((s, id) => s + (orderPos.get(id) ?? 0), 0) / na.length : orderPos.get(a) ?? 0;
      const avgB = nb.length ? nb.reduce((s, id) => s + (orderPos.get(id) ?? 0), 0) / nb.length : orderPos.get(b) ?? 0;
      if (Math.abs(avgA - avgB) > 0.001) return avgA - avgB;
      return (indexOf.get(a) ?? 0) - (indexOf.get(b) ?? 0);
    });
  };

  for (let pass = 0; pass < 2; pass++) {
    for (let l = 1; l < layers.length; l++) {
      sortByNeighbors(layers[l]!, parentsOf);
      refreshPos();
    }
    for (let l = layers.length - 2; l >= 0; l--) {
      sortByNeighbors(layers[l]!, childrenOf);
      refreshPos();
    }
  }

  const maxLayerSize = Math.max(...layers.map((l) => l.length));
  const direction: FlowDirection =
    block.direction ?? (layers.length <= 3 && maxLayerSize <= 3 ? "lr" : "td");

  const nodeById = new Map(nodesInput.map((n) => [n.id, n]));
  const sizes = new Map<string, { w: number; h: number }>();
  for (const id of nodeIds) {
    const n = nodeById.get(id)!;
    sizes.set(id, measureNode(n.label, n.note, direction));
  }

  // Check if any edge crossing a gap between layer l and l+1 carries a label.
  const gapHasLabel = new Array<boolean>(Math.max(0, layers.length - 1)).fill(false);
  validEdges.forEach((e, idx) => {
    if (backEdges.has(idx) || !e.label) return;
    const lf = layerMap.get(e.from) ?? 0;
    const lt = layerMap.get(e.to) ?? 0;
    if (lt > lf && lf < gapHasLabel.length) {
      gapHasLabel[lf] = true;
    }
  });

  const boxes = new Map<string, FlowNodeBox>();
  let totalW = 0;
  let totalH = 0;

  if (direction === "td") {
    const layerWidths = layers.map((arr) => {
      const sumW = arr.reduce((s, id) => s + sizes.get(id)!.w, 0);
      return sumW + Math.max(0, arr.length - 1) * SIBLING_GAP_TD;
    });
    const contentW = Math.max(...layerWidths, 120);
    let y = PAD;

    layers.forEach((arr, l) => {
      const layerH = Math.max(...arr.map((id) => sizes.get(id)!.h), 32);
      const rowW = layerWidths[l]!;
      let x = PAD + Math.round((contentW - rowW) / 2);

      arr.forEach((id, order) => {
        const n = nodeById.get(id)!;
        const sz = sizes.get(id)!;
        boxes.set(id, {
          id,
          label: n.label,
          note: n.note,
          tone: n.tone ?? "default",
          layer: l,
          order,
          x,
          y: y + Math.round((layerH - sz.h) / 2),
          w: sz.w,
          h: sz.h,
        });
        x += sz.w + SIBLING_GAP_TD;
      });

      y += layerH + (l < layers.length - 1 ? LAYER_GAP_TD + (gapHasLabel[l] ? LABEL_GAP_BONUS : 0) : 0);
    });

    totalW = contentW + PAD * 2;
    totalH = y + PAD;
  } else {
    const layerHeights = layers.map((arr) => {
      const sumH = arr.reduce((s, id) => s + sizes.get(id)!.h, 0);
      return sumH + Math.max(0, arr.length - 1) * SIBLING_GAP_LR;
    });
    const contentH = Math.max(...layerHeights, 48);
    let x = PAD;

    layers.forEach((arr, l) => {
      const layerW = Math.max(...arr.map((id) => sizes.get(id)!.w), 84);
      const colH = layerHeights[l]!;
      let y = PAD + Math.round((contentH - colH) / 2);

      arr.forEach((id, order) => {
        const n = nodeById.get(id)!;
        const sz = sizes.get(id)!;
        boxes.set(id, {
          id,
          label: n.label,
          note: n.note,
          tone: n.tone ?? "default",
          layer: l,
          order,
          x: x + Math.round((layerW - sz.w) / 2),
          y,
          w: sz.w,
          h: sz.h,
        });
        y += sz.h + SIBLING_GAP_LR;
      });

      x += layerW + (l < layers.length - 1 ? LAYER_GAP_LR + (gapHasLabel[l] ? LABEL_GAP_BONUS : 0) : 0);
    });

    totalW = x + PAD;
    totalH = contentH + PAD * 2;
  }

  let extraRight = 0;
  let extraBottom = 0;
  let backCount = 0;

  const routed: FlowEdgeRoute[] = validEdges.map((e, idx) => {
    const a = boxes.get(e.from)!;
    const b = boxes.get(e.to)!;
    const isBack = backEdges.has(idx) || e.from === e.to || b.layer <= a.layer;

    if (e.from === e.to) {
      // Neat self-loop on the right side.
      const sx = round1(a.x + a.w);
      const sy = round1(a.y + a.h * 0.3);
      const ex = round1(a.x + a.w);
      const ey = round1(a.y + a.h * 0.72);
      const cx = round1(sx + 26);
      extraRight = Math.max(extraRight, 28);
      return {
        from: e.from,
        to: e.to,
        label: e.label,
        path: `M ${sx} ${sy} C ${cx} ${sy - 10}, ${cx} ${ey + 10}, ${ex} ${ey}`,
        labelPos: e.label ? { x: round1(cx + 2), y: round1((sy + ey) / 2) } : undefined,
        backEdge: true,
      };
    }

    if (isBack) {
      backCount += 1;
      const lane = 16 + (backCount - 1) * 14;
      if (direction === "td") {
        const sx = round1(a.x + a.w);
        const sy = round1(a.y + a.h / 2);
        const ex = round1(b.x + b.w);
        const ey = round1(b.y + b.h / 2);
        const cx = round1(totalW - PAD + lane);
        extraRight = Math.max(extraRight, lane + 14);
        const path = `M ${sx} ${sy} C ${cx} ${sy}, ${cx} ${ey}, ${ex} ${ey}`;
        return {
          from: e.from,
          to: e.to,
          label: e.label,
          path,
          labelPos: e.label ? cubicPoint(0.5, sx, sy, cx, sy, cx, ey, ex, ey) : undefined,
          backEdge: true,
        };
      } else {
        const sx = round1(a.x + a.w / 2);
        const sy = round1(a.y + a.h);
        const ex = round1(b.x + b.w / 2);
        const ey = round1(b.y + b.h);
        const cy = round1(totalH - PAD + lane);
        extraBottom = Math.max(extraBottom, lane + 14);
        const path = `M ${sx} ${sy} C ${sx} ${cy}, ${ex} ${cy}, ${ex} ${ey}`;
        return {
          from: e.from,
          to: e.to,
          label: e.label,
          path,
          labelPos: e.label ? cubicPoint(0.5, sx, sy, sx, cy, ex, cy, ex, ey) : undefined,
          backEdge: true,
        };
      }
    }

    // Forward DAG edge.
    const span = b.layer - a.layer;
    if (direction === "td") {
      const sx = round1(a.x + a.w / 2);
      const sy = round1(a.y + a.h);
      const ex = round1(b.x + b.w / 2);
      const ey = round1(b.y);
      const dy = Math.max(16, (ey - sy) * 0.45);
      // Long skip edges bow slightly outward to clear intermediate nodes.
      const bow = span > 1 ? (sx <= totalW / 2 ? -24 : 24) : 0;
      const c1x = round1(sx + bow);
      const c1y = round1(sy + dy);
      const c2x = round1(ex + bow);
      const c2y = round1(ey - dy);
      return {
        from: e.from,
        to: e.to,
        label: e.label,
        path: `M ${sx} ${sy} C ${c1x} ${c1y}, ${c2x} ${c2y}, ${ex} ${ey}`,
        labelPos: e.label ? cubicPoint(0.5, sx, sy, c1x, c1y, c2x, c2y, ex, ey) : undefined,
        backEdge: false,
      };
    } else {
      const sx = round1(a.x + a.w);
      const sy = round1(a.y + a.h / 2);
      const ex = round1(b.x);
      const ey = round1(b.y + b.h / 2);
      const dx = Math.max(18, (ex - sx) * 0.45);
      const bow = span > 1 ? (sy <= totalH / 2 ? -22 : 22) : 0;
      const c1x = round1(sx + dx);
      const c1y = round1(sy + bow);
      const c2x = round1(ex - dx);
      const c2y = round1(ey + bow);
      return {
        from: e.from,
        to: e.to,
        label: e.label,
        path: `M ${sx} ${sy} C ${c1x} ${c1y}, ${c2x} ${c2y}, ${ex} ${ey}`,
        labelPos: e.label ? cubicPoint(0.5, sx, sy, c1x, c1y, c2x, c2y, ex, ey) : undefined,
        backEdge: false,
      };
    }
  });

  const orderedNodes = nodeIds.map((id) => boxes.get(id)!);
  return {
    direction,
    width: Math.round(totalW + extraRight),
    height: Math.round(totalH + extraBottom),
    nodes: orderedNodes,
    edges: routed,
  };
}
