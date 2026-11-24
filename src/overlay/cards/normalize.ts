// Tolerant validation and clamping for LLM-generated explanation cards.
// Accepts untrusted JSON, coerces common model mistakes (numeric strings,
// alternative field names, oversized lists) and guarantees clean renderable
// data or returns null if nothing usable remains.

import {
  CARD_LIMITS,
  type BarItem,
  type BarsBlock,
  type Block,
  type CalloutBlock,
  type CalloutTone,
  type CardSpec,
  type CompareBlock,
  type CompareColumn,
  type CompareItem,
  type CompareItemKind,
  type DefinitionBlock,
  type FlowBlock,
  type FlowDirection,
  type FlowEdge,
  type FlowNode,
  type FlowTone,
  type KvBlock,
  type KvItem,
  type LineBlock,
  type LineSeries,
  type MathBlock,
  type MathLine,
  type QuizBlock,
  type StepItem,
  type StepsBlock,
  type TableBlock,
  type TextBlock,
  type TimelineBlock,
  type TimelineItem,
} from "./types";

type Obj = Record<string, unknown>;

function isObj(v: unknown): v is Obj {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function clampStr(v: unknown, max: number): string {
  if (typeof v === "number" && Number.isFinite(v)) return String(v);
  if (typeof v === "boolean") return v ? "Да" : "Нет";
  if (typeof v !== "string") return "";
  const trimmed = v.trim();
  if (!trimmed) return "";
  const chars = Array.from(trimmed);
  if (chars.length <= max) return trimmed;
  return `${chars.slice(0, Math.max(1, max - 1)).join("").trimEnd()}…`;
}

function optStr(v: unknown, max: number): string | undefined {
  const s = clampStr(v, max);
  return s ? s : undefined;
}

function toNum(v: unknown): number | null {
  if (typeof v === "number") return Number.isFinite(v) ? v : null;
  if (typeof v === "string") {
    const cleaned = v.trim().replace(/\s+/g, "").replace(",", ".");
    if (!cleaned) return null;
    const n = Number(cleaned);
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

function toBool(v: unknown): boolean {
  if (typeof v === "boolean") return v;
  if (typeof v === "string") return v.trim().toLowerCase() === "true";
  return false;
}

function normText(raw: Obj): TextBlock | null {
  const text = clampStr(raw.text ?? raw.content ?? raw.body, CARD_LIMITS.maxText);
  return text ? { kind: "text", text } : null;
}

function normSteps(raw: Obj): StepsBlock | null {
  const src = Array.isArray(raw.items) ? raw.items : Array.isArray(raw.steps) ? raw.steps : [];
  const items: StepItem[] = [];
  for (const it of src) {
    if (items.length >= CARD_LIMITS.maxSteps) break;
    if (typeof it === "string") {
      const title = clampStr(it, CARD_LIMITS.maxShortText);
      if (title) items.push({ title });
    } else if (isObj(it)) {
      const title = clampStr(it.title ?? it.label ?? it.text, CARD_LIMITS.maxShortText);
      if (!title) continue;
      const note = optStr(it.note ?? it.detail ?? it.description ?? it.sub, CARD_LIMITS.maxShortText);
      items.push(note ? { title, note } : { title });
    }
  }
  return items.length > 0 ? { kind: "steps", items } : null;
}

function normTable(raw: Obj): TableBlock | null {
  const rawHeaders = Array.isArray(raw.headers) ? raw.headers : Array.isArray(raw.columns) ? raw.columns : [];
  const headers = rawHeaders
    .slice(0, CARD_LIMITS.maxTableCols)
    .map((h) => clampStr(h, CARD_LIMITS.maxCellText))
    .filter((h) => h.length > 0);
  if (headers.length === 0) return null;

  const rawRows = Array.isArray(raw.rows) ? raw.rows : [];
  const rows: string[][] = [];
  for (const r of rawRows) {
    if (rows.length >= CARD_LIMITS.maxTableRows) break;
    if (!Array.isArray(r)) continue;
    const cells: string[] = [];
    for (let c = 0; c < headers.length; c++) {
      cells.push(clampStr(r[c], CARD_LIMITS.maxCellText));
    }
    if (cells.some((c) => c.length > 0)) rows.push(cells);
  }
  if (rows.length === 0) return null;

  const hlRaw = toNum(raw.highlightCol ?? raw.highlight_col);
  const highlightCol =
    hlRaw !== null && Number.isInteger(hlRaw) && hlRaw >= 0 && hlRaw < headers.length ? hlRaw : undefined;

  return highlightCol !== undefined
    ? { kind: "table", headers, rows, highlightCol }
    : { kind: "table", headers, rows };
}

function normBars(raw: Obj): BarsBlock | null {
  const src = Array.isArray(raw.items) ? raw.items : Array.isArray(raw.bars) ? raw.bars : [];
  const items: BarItem[] = [];
  for (const it of src) {
    if (items.length >= CARD_LIMITS.maxBars) break;
    if (!isObj(it)) continue;
    const label = clampStr(it.label ?? it.name ?? it.key, CARD_LIMITS.maxCellText);
    const value = toNum(it.value ?? it.val ?? it.y);
    if (!label || value === null) continue;
    const highlight = toBool(it.highlight);
    items.push(highlight ? { label, value, highlight: true } : { label, value });
  }
  if (items.length === 0) return null;
  const title = optStr(raw.title, CARD_LIMITS.maxCellText);
  const unit = optStr(raw.unit ?? raw.units, 24);
  const out: BarsBlock = { kind: "bars", items };
  if (title) out.title = title;
  if (unit) out.unit = unit;
  return out;
}

function normLine(raw: Obj): LineBlock | null {
  const title = optStr(raw.title, CARD_LIMITS.maxCellText);
  const unit = optStr(raw.unit ?? raw.units, 24);
  let xLabels: string[] = [];
  const series: LineSeries[] = [];

  if (Array.isArray(raw.points) && !Array.isArray(raw.series)) {
    const vals: number[] = [];
    for (const pt of raw.points) {
      if (vals.length >= CARD_LIMITS.maxLinePoints) break;
      if (!isObj(pt)) continue;
      const y = toNum(pt.y ?? pt.value);
      if (y === null) continue;
      xLabels.push(clampStr(pt.x ?? pt.label ?? String(vals.length + 1), 24) || String(vals.length + 1));
      vals.push(y);
    }
    if (vals.length >= 2) series.push({ values: vals });
  } else {
    const rawLabels = Array.isArray(raw.xLabels)
      ? raw.xLabels
      : Array.isArray(raw.x_labels)
        ? raw.x_labels
        : Array.isArray(raw.labels)
          ? raw.labels
          : [];
    xLabels = rawLabels.slice(0, CARD_LIMITS.maxLinePoints).map((l, idx) => clampStr(l, 24) || String(idx + 1));

    if (Array.isArray(raw.series)) {
      for (const s of raw.series) {
        if (series.length >= CARD_LIMITS.maxLineSeries) break;
        if (!isObj(s)) continue;
        const srcVals = Array.isArray(s.values) ? s.values : Array.isArray(s.points) ? s.points : [];
        const values: number[] = [];
        for (const v of srcVals) {
          if (values.length >= CARD_LIMITS.maxLinePoints) break;
          const n = toNum(v);
          if (n !== null) values.push(n);
        }
        if (values.length < 2) continue;
        const name = optStr(s.name ?? s.label, 48);
        series.push(name ? { name, values } : { values });
      }
    } else if (Array.isArray(raw.values)) {
      const values: number[] = [];
      for (const v of raw.values) {
        if (values.length >= CARD_LIMITS.maxLinePoints) break;
        const n = toNum(v);
        if (n !== null) values.push(n);
      }
      if (values.length >= 2) series.push({ values });
    }
  }

  if (series.length === 0) return null;
  const maxLen = Math.min(CARD_LIMITS.maxLinePoints, Math.max(...series.map((s) => s.values.length)));
  while (xLabels.length < maxLen) xLabels.push(String(xLabels.length + 1));
  if (xLabels.length > maxLen) xLabels = xLabels.slice(0, maxLen);

  const out: LineBlock = { kind: "line", xLabels, series };
  if (title) out.title = title;
  if (unit) out.unit = unit;
  return out;
}

function normTimeline(raw: Obj): TimelineBlock | null {
  const src = Array.isArray(raw.items) ? raw.items : Array.isArray(raw.events) ? raw.events : [];
  const items: TimelineItem[] = [];
  for (const it of src) {
    if (items.length >= CARD_LIMITS.maxTimeline) break;
    if (!isObj(it)) continue;
    const date = clampStr(it.date ?? it.time ?? it.year, 40);
    const label = clampStr(it.label ?? it.title ?? it.event, CARD_LIMITS.maxShortText);
    if (!date || !label) continue;
    const note = optStr(it.note ?? it.detail ?? it.description, CARD_LIMITS.maxShortText);
    items.push(note ? { date, label, note } : { date, label });
  }
  return items.length > 0 ? { kind: "timeline", items } : null;
}

function normFlow(raw: Obj): FlowBlock | null {
  const rawNodes = Array.isArray(raw.nodes) ? raw.nodes : [];
  const nodes: FlowNode[] = [];
  const seen = new Set<string>();

  for (const n of rawNodes) {
    if (nodes.length >= CARD_LIMITS.maxFlowNodes) break;
    if (!isObj(n)) continue;
    const id = clampStr(n.id, 48);
    const label = clampStr(n.label ?? n.title ?? n.text ?? id, CARD_LIMITS.maxCellText);
    if (!id || !label || seen.has(id)) continue;
    seen.add(id);
    const note = optStr(n.note ?? n.sub, CARD_LIMITS.maxCellText);
    const toneRaw = clampStr(n.tone, 16);
    const tone: FlowTone | undefined =
      toneRaw === "accent" || toneRaw === "muted" || toneRaw === "default" ? toneRaw : undefined;
    const node: FlowNode = { id, label };
    if (note) node.note = note;
    if (tone && tone !== "default") node.tone = tone;
    nodes.push(node);
  }
  if (nodes.length === 0) return null;

  const rawEdges = Array.isArray(raw.edges) ? raw.edges : [];
  const edges: FlowEdge[] = [];
  const edgeKeys = new Set<string>();
  for (const e of rawEdges) {
    if (edges.length >= CARD_LIMITS.maxFlowEdges) break;
    if (!isObj(e)) continue;
    const from = clampStr(e.from ?? e.source, 48);
    const to = clampStr(e.to ?? e.target, 48);
    if (!from || !to || !seen.has(from) || !seen.has(to)) continue;
    const label = optStr(e.label, 48);
    const key = `${from}->${to}:${label ?? ""}`;
    if (edgeKeys.has(key)) continue;
    edgeKeys.add(key);
    edges.push(label ? { from, to, label } : { from, to });
  }

  const dirRaw = clampStr(raw.direction, 8).toLowerCase();
  const direction: FlowDirection | undefined = dirRaw === "lr" || dirRaw === "td" ? dirRaw : undefined;
  const out: FlowBlock = { kind: "flow", nodes, edges };
  if (direction) out.direction = direction;
  return out;
}

function normCompareKind(v: unknown): CompareItemKind | undefined {
  const s = clampStr(v, 16).toLowerCase();
  if (s === "pro" || s === "plus" || s === "+") return "pro";
  if (s === "con" || s === "minus" || s === "-") return "con";
  if (s === "neutral") return "neutral";
  return undefined;
}

function normCompare(raw: Obj): CompareBlock | null {
  const src = Array.isArray(raw.columns) ? raw.columns : Array.isArray(raw.items) ? raw.items : [];
  const columns: CompareColumn[] = [];
  for (const col of src) {
    if (columns.length >= CARD_LIMITS.maxCompareCols) break;
    if (!isObj(col)) continue;
    const title = clampStr(col.title ?? col.name, CARD_LIMITS.maxCellText);
    if (!title) continue;
    const subtitle = optStr(col.subtitle ?? col.sub, CARD_LIMITS.maxCellText);
    const accent = toBool(col.accent);
    const items: CompareItem[] = [];

    if (Array.isArray(col.items)) {
      for (const it of col.items) {
        if (items.length >= CARD_LIMITS.maxCompareItems) break;
        if (typeof it === "string") {
          const text = clampStr(it, CARD_LIMITS.maxShortText);
          if (text) items.push({ text });
        } else if (isObj(it)) {
          const text = clampStr(it.text ?? it.label, CARD_LIMITS.maxShortText);
          if (!text) continue;
          const kind = normCompareKind(it.kind ?? it.type);
          items.push(kind ? { text, kind } : { text });
        }
      }
    } else {
      // Tolerate `{ pros: [...], cons: [...] }` shape.
      if (Array.isArray(col.pros)) {
        for (const p of col.pros) {
          if (items.length >= CARD_LIMITS.maxCompareItems) break;
          const text = clampStr(p, CARD_LIMITS.maxShortText);
          if (text) items.push({ text, kind: "pro" });
        }
      }
      if (Array.isArray(col.cons)) {
        for (const c of col.cons) {
          if (items.length >= CARD_LIMITS.maxCompareItems) break;
          const text = clampStr(c, CARD_LIMITS.maxShortText);
          if (text) items.push({ text, kind: "con" });
        }
      }
    }

    if (items.length === 0) continue;
    const normalized: CompareColumn = { title, items };
    if (subtitle) normalized.subtitle = subtitle;
    if (accent) normalized.accent = true;
    columns.push(normalized);
  }
  return columns.length > 0 ? { kind: "compare", columns } : null;
}

function normMath(raw: Obj): MathBlock | null {
  const src = Array.isArray(raw.lines) ? raw.lines : Array.isArray(raw.steps) ? raw.steps : [];
  const lines: MathLine[] = [];
  for (const it of src) {
    if (lines.length >= CARD_LIMITS.maxMathLines) break;
    if (typeof it === "string") {
      const expr = clampStr(it, CARD_LIMITS.maxShortText);
      if (expr) lines.push({ expr });
    } else if (isObj(it)) {
      const expr = clampStr(it.expr ?? it.expression ?? it.formula ?? it.text, CARD_LIMITS.maxShortText);
      if (!expr) continue;
      const note = optStr(it.note ?? it.label ?? it.comment, CARD_LIMITS.maxShortText);
      lines.push(note ? { expr, note } : { expr });
    }
  }
  if (lines.length === 0) return null;
  const result = optStr(raw.result ?? raw.answer, CARD_LIMITS.maxShortText);
  return result ? { kind: "math", lines, result } : { kind: "math", lines };
}

function normDefinition(raw: Obj): DefinitionBlock | null {
  const term = clampStr(raw.term ?? raw.title ?? raw.word, CARD_LIMITS.maxCellText);
  const meaning = clampStr(raw.meaning ?? raw.definition ?? raw.text, CARD_LIMITS.maxText);
  if (!term || !meaning) return null;
  const example = optStr(raw.example, CARD_LIMITS.maxShortText);
  return example ? { kind: "definition", term, meaning, example } : { kind: "definition", term, meaning };
}

function normQuiz(raw: Obj): QuizBlock | null {
  const question = clampStr(raw.question ?? raw.prompt, CARD_LIMITS.maxShortText);
  const rawOptions = Array.isArray(raw.options) ? raw.options : Array.isArray(raw.choices) ? raw.choices : [];
  const options: string[] = [];
  for (const opt of rawOptions) {
    if (options.length >= CARD_LIMITS.maxQuizOptions) break;
    const text = clampStr(opt, CARD_LIMITS.maxShortText);
    if (text) options.push(text);
  }
  if (!question || options.length < 2) return null;

  const rawCorrect = toNum(raw.correct ?? raw.correctIndex ?? raw.correct_index ?? raw.answer);
  const correct =
    rawCorrect !== null && Number.isInteger(rawCorrect)
      ? Math.max(0, Math.min(options.length - 1, rawCorrect))
      : 0;
  const explanation = optStr(raw.explanation ?? raw.note, CARD_LIMITS.maxText);
  return explanation
    ? { kind: "quiz", question, options, correct, explanation }
    : { kind: "quiz", question, options, correct };
}

function normCallout(raw: Obj): CalloutBlock | null {
  const text = clampStr(raw.text ?? raw.body ?? raw.content, CARD_LIMITS.maxText);
  if (!text) return null;
  const toneRaw = clampStr(raw.tone ?? raw.variant, 16).toLowerCase();
  const tone: CalloutTone = toneRaw === "tip" || toneRaw === "warning" || toneRaw === "idea" ? toneRaw : "idea";
  const title = optStr(raw.title, CARD_LIMITS.maxCellText);
  const out: CalloutBlock = { kind: "callout", tone, text };
  if (title) out.title = title;
  return out;
}

function normKv(raw: Obj): KvBlock | null {
  const src = Array.isArray(raw.items) ? raw.items : Array.isArray(raw.pairs) ? raw.pairs : [];
  const items: KvItem[] = [];
  for (const it of src) {
    if (items.length >= CARD_LIMITS.maxKvItems) break;
    if (!isObj(it)) continue;
    const key = clampStr(it.key ?? it.label ?? it.name, CARD_LIMITS.maxCellText);
    const value = clampStr(it.value ?? it.val, CARD_LIMITS.maxShortText);
    if (!key || !value) continue;
    items.push({ key, value });
  }
  return items.length > 0 ? { kind: "kv", items } : null;
}

function normBlock(raw: unknown): Block | null {
  if (!isObj(raw)) return null;
  const kind = clampStr(raw.kind ?? raw.type, 24).toLowerCase();
  switch (kind) {
    case "text":
      return normText(raw);
    case "steps":
      return normSteps(raw);
    case "table":
      return normTable(raw);
    case "bars":
      return normBars(raw);
    case "line":
      return normLine(raw);
    case "timeline":
      return normTimeline(raw);
    case "flow":
      return normFlow(raw);
    case "compare":
      return normCompare(raw);
    case "math":
      return normMath(raw);
    case "definition":
      return normDefinition(raw);
    case "quiz":
      return normQuiz(raw);
    case "callout":
      return normCallout(raw);
    case "kv":
      return normKv(raw);
    default:
      return null;
  }
}

export function normalizeCard(raw: unknown): CardSpec | null {
  if (!isObj(raw)) return null;
  const title = clampStr(raw.title, CARD_LIMITS.maxTitle);
  if (!title) return null;
  const rawBlocks = Array.isArray(raw.blocks) ? raw.blocks : [];
  const blocks: Block[] = [];
  for (const b of rawBlocks) {
    if (blocks.length >= CARD_LIMITS.maxBlocks) break;
    const normalized = normBlock(b);
    if (normalized) blocks.push(normalized);
  }
  if (blocks.length === 0) return null;
  const subtitle = optStr(raw.subtitle, CARD_LIMITS.maxSubtitle);
  return subtitle ? { title, subtitle, blocks } : { title, blocks };
}
