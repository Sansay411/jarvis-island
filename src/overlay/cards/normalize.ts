// Tolerant validation and clamping for model-written explanation cards.
// Accepts untrusted JSON, coerces common model mistakes (numeric strings,
// alternative field names and kind names, stringified arrays, tuples instead
// of objects, oversized lists) and returns clean renderable data, or null if
// nothing usable remains. Mirrors `src-tauri/src/cards.rs`; keep them in step.

import {
  CARD_LIMITS,
  type BarItem,
  type Block,
  type CalloutTone,
  type CardSpec,
  type CompareColumn,
  type CompareItem,
  type CompareItemKind,
  type FlowDirection,
  type FlowEdge,
  type FlowNode,
  type KvItem,
  type LineSeries,
  type MathLine,
  type StepItem,
  type TimelineItem,
} from "./types";

type Obj = Record<string, unknown>;

/** Title used when the model forgot one but the blocks are fine. */
export const FALLBACK_TITLE = "Пояснение";

/** Raw blocks looked at, at most (later ones are ignored before clamping). */
const MAX_RAW_BLOCKS = 32;

function isObj(v: unknown): v is Obj {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** Drops control characters (keeps new lines and tabs) and trims. */
function cleanText(s: string): string {
  // eslint-disable-next-line no-control-regex
  return s.replace(/\r\n?/g, "\n").replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, "").trim();
}

export function clampStr(v: unknown, max: number): string {
  let raw: string;
  if (typeof v === "number") {
    if (!Number.isFinite(v)) return "";
    raw = String(v);
  } else if (typeof v === "boolean") raw = v ? "Да" : "Нет";
  else if (typeof v === "string") raw = cleanText(v);
  else return "";
  if (!raw) return "";
  const chars = Array.from(raw);
  if (chars.length <= max) return raw;
  return `${chars.slice(0, Math.max(1, max - 1)).join("").trimEnd()}…`;
}

/** First non-empty string among `keys`. */
function pick(o: Obj, keys: readonly string[], max: number): string {
  for (const k of keys) {
    const s = clampStr(o[k], max);
    if (s) return s;
  }
  return "";
}

function optPick(o: Obj, keys: readonly string[], max: number): string | undefined {
  return pick(o, keys, max) || undefined;
}

/** First value among `keys` that is not null/undefined. */
function pickRaw(o: Obj, keys: readonly string[]): unknown {
  for (const k of keys) {
    if (o[k] !== undefined && o[k] !== null) return o[k];
  }
  return undefined;
}

/** Arrays pass through; a string holding a JSON array is parsed. */
function asArr(v: unknown): unknown[] {
  if (Array.isArray(v)) return v;
  if (typeof v === "string") {
    const s = v.trim();
    if (s.startsWith("[")) {
      try {
        const parsed: unknown = JSON.parse(s);
        if (Array.isArray(parsed)) return parsed;
      } catch {
        /* not JSON */
      }
    }
  }
  return [];
}

/** First non-empty array among `keys`. */
function pickArr(o: Obj, keys: readonly string[]): unknown[] {
  for (const k of keys) {
    const a = asArr(o[k]);
    if (a.length > 0) return a;
  }
  return [];
}

/** A plain object used as a map (`{"Порт": 443}`) as ordered pairs. */
function mapEntries(v: unknown): [string, unknown][] {
  return isObj(v) ? Object.entries(v) : [];
}

/**
 * Reads a number from model output: `42`, `"42,5"`, `"1 200 ₽"`, `"$1,234.5"`,
 * `"−7 %"`. The first number in a string wins; a lone comma is a decimal
 * mark unless exactly three digits follow it (`"1,500"` is 1500).
 */
export function parseNumber(v: unknown): number | null {
  if (typeof v === "number") return Number.isFinite(v) ? v : null;
  if (typeof v !== "string") return null;
  const s = v.replace(/[\u2212\u2012\u2013\u2014]/g, "-");
  const m = /([-+]?)(\d[\d\s\u00a0\u202f.,']*)/.exec(s);
  if (!m) return null;
  let t = m[2]!.replace(/[\s\u00a0\u202f']/g, "").replace(/[.,]+$/, "");
  const dots = (t.match(/\./g) ?? []).length;
  const commas = (t.match(/,/g) ?? []).length;
  if (dots > 0 && commas > 0) {
    const decimal = t.lastIndexOf(".") > t.lastIndexOf(",") ? "." : ",";
    const thousands = decimal === "." ? "," : ".";
    t = t.split(thousands).join("").replace(",", ".");
  } else if (commas > 1) {
    t = t.replace(/,/g, "");
  } else if (commas === 1) {
    const [int = "", frac = ""] = t.split(",");
    t = frac.length === 3 && int !== "0" ? int + frac : `${int}.${frac}`;
  } else if (dots > 1) {
    t = t.replace(/\./g, "");
  }
  const n = Number((m[1] === "-" ? "-" : "") + t);
  return Number.isFinite(n) ? n : null;
}

function toBool(v: unknown): boolean {
  if (typeof v === "boolean") return v;
  if (typeof v === "number") return v === 1;
  if (typeof v === "string") return ["true", "yes", "да", "1"].includes(v.trim().toLowerCase());
  return false;
}

function key(s: string): string {
  return s.toLowerCase().replace(/[\s_-]+/g, "");
}

// ---------- kinds ----------

type Kind = Block["kind"];

const KIND_ALIASES: Record<string, Kind> = {
  text: "text",
  markdown: "text",
  md: "text",
  paragraph: "text",
  summary: "text",
  steps: "steps",
  step: "steps",
  list: "steps",
  process: "steps",
  howto: "steps",
  instructions: "steps",
  checklist: "steps",
  table: "table",
  grid: "table",
  bars: "bars",
  bar: "bars",
  barchart: "bars",
  histogram: "bars",
  line: "line",
  linechart: "line",
  trend: "line",
  timeline: "timeline",
  chronology: "timeline",
  history: "timeline",
  flow: "flow",
  flowchart: "flow",
  diagram: "flow",
  compare: "compare",
  comparison: "compare",
  versus: "compare",
  vs: "compare",
  proscons: "compare",
  math: "math",
  formula: "math",
  equation: "math",
  solution: "math",
  calculation: "math",
  definition: "definition",
  define: "definition",
  term: "definition",
  glossary: "definition",
  quiz: "quiz",
  question: "quiz",
  test: "quiz",
  callout: "callout",
  tip: "callout",
  hint: "callout",
  note: "callout",
  idea: "callout",
  info: "callout",
  warning: "callout",
  warn: "callout",
  alert: "callout",
  important: "callout",
  kv: "kv",
  keyvalue: "kv",
  facts: "kv",
  specs: "kv",
  properties: "kv",
  stats: "kv",
};

/** Guesses the kind from the fields when the name is missing or unknown. */
function inferKind(o: Obj): Kind | null {
  const has = (k: string) => o[k] !== undefined && o[k] !== null;
  if (has("nodes") || has("edges")) return "flow";
  if (has("question") && (has("options") || has("choices") || has("answers"))) return "quiz";
  if (has("headers") || has("rows")) return "table";
  if (has("series") || has("xLabels") || has("x_labels") || has("points")) return "line";
  if (has("term") && (has("meaning") || has("definition"))) return "definition";
  if (has("lines") || has("expr") || has("formula")) return "math";
  if (has("columns")) return "compare";
  const items = pickArr(o, ["items", "bars", "data", "events", "pairs", "steps"]);
  const firstItem = items[0];
  if (isObj(firstItem)) {
    if (pickRaw(firstItem, ["date", "year", "time", "when"]) !== undefined) return "timeline";
    const val = pickRaw(firstItem, ["value", "val", "y", "count", "amount"]);
    if (val !== undefined) return parseNumber(val) !== null && typeof val !== "boolean" ? "bars" : "kv";
    if (pickRaw(firstItem, ["key", "k"]) !== undefined) return "kv";
    return "steps";
  }
  if (items.length > 0) return "steps";
  if (has("tone") || has("variant")) return "callout";
  if (has("text") || has("content") || has("body") || has("markdown")) return "text";
  return null;
}

function resolveKind(o: Obj): { kind: Kind; alias: string } | null {
  const alias = key(clampStr(o.kind ?? o.type ?? o.block, 32));
  const known = KIND_ALIASES[alias];
  if (known) return { kind: known, alias };
  // "chart"/"graph" and anything unknown: look at the data instead.
  const inferred = inferKind(o);
  return inferred ? { kind: inferred, alias } : null;
}

// ---------- blocks ----------

const TITLE_KEYS = ["title", "label", "name", "text", "heading", "step"] as const;
const NOTE_KEYS = ["note", "detail", "details", "description", "sub", "subtitle", "desc"] as const;

function normText(o: Obj): Block | null {
  let text = pick(o, ["text", "content", "body", "markdown", "value", "message"], CARD_LIMITS.maxText);
  if (!text) {
    const lines = pickArr(o, ["items", "lines", "bullets", "points"])
      .map((it) => clampStr(it, CARD_LIMITS.maxShortText))
      .filter(Boolean);
    text = clampStr(lines.map((l) => `- ${l}`).join("\n"), CARD_LIMITS.maxText);
  }
  return text ? { kind: "text", text } : null;
}

function normSteps(o: Obj): Block | null {
  const items: StepItem[] = [];
  for (const it of pickArr(o, ["items", "steps", "list", "points"])) {
    if (items.length >= CARD_LIMITS.maxSteps) break;
    if (isObj(it)) {
      const title = pick(it, TITLE_KEYS, CARD_LIMITS.maxShortText);
      if (!title) continue;
      const note = optPick(it, NOTE_KEYS, CARD_LIMITS.maxShortText);
      items.push(note && note !== title ? { title, note } : { title });
    } else {
      const title = clampStr(it, CARD_LIMITS.maxShortText);
      if (title) items.push({ title });
    }
  }
  return items.length > 0 ? { kind: "steps", items } : null;
}

function normTable(o: Obj): Block | null {
  const rawRows = pickArr(o, ["rows", "data", "items"]);
  let headers = pickArr(o, ["headers", "columns", "cols", "header"]).map((h) =>
    clampStr(isObj(h) ? pickRaw(h, ["title", "label", "name", "text"]) : h, CARD_LIMITS.maxCellText),
  );
  const firstRow = rawRows.find((r) => isObj(r));
  if (!headers.some(Boolean) && isObj(firstRow)) headers = Object.keys(firstRow).map((k) => clampStr(k, CARD_LIMITS.maxCellText));

  const rowArr = (r: unknown): unknown[] | null => {
    if (Array.isArray(r)) return r;
    if (typeof r === "string") return r.includes("|") ? r.split("|").map((c) => c.trim()).filter((c, i, a) => c || (i > 0 && i < a.length - 1)) : [r];
    return null;
  };
  const widest = Math.max(0, ...rawRows.map((r) => rowArr(r)?.length ?? 0));
  const cols = Math.min(CARD_LIMITS.maxTableCols, Math.max(headers.length, headers.some(Boolean) ? 0 : widest));
  if (cols === 0) return null;
  headers = headers.slice(0, cols);
  while (headers.length < cols) headers.push("");

  const rows: string[][] = [];
  for (const r of rawRows) {
    if (rows.length >= CARD_LIMITS.maxTableRows) break;
    let cells: string[];
    if (isObj(r)) {
      const lower = new Map(Object.keys(r).map((k) => [key(k), k]));
      const values = Object.values(r);
      cells = headers.map((h, ci) => {
        const k = h ? lower.get(key(h)) : undefined;
        return clampStr(k !== undefined ? r[k] : values[ci], CARD_LIMITS.maxCellText);
      });
    } else {
      const arr = rowArr(r);
      if (!arr) continue;
      cells = headers.map((_, ci) => clampStr(arr[ci], CARD_LIMITS.maxCellText));
    }
    if (cells.some(Boolean)) rows.push(cells);
  }
  if (rows.length === 0) return null;

  const hlRaw = pickRaw(o, ["highlightCol", "highlight_col", "highlight", "highlightColumn"]);
  let highlightCol: number | undefined;
  const hlNum = typeof hlRaw === "boolean" ? null : parseNumber(hlRaw);
  if (hlNum !== null && Number.isInteger(hlNum) && hlNum >= 0 && hlNum < cols) highlightCol = hlNum;
  else if (typeof hlRaw === "string" && hlNum === null) {
    const idx = headers.findIndex((h) => h && key(h) === key(hlRaw));
    if (idx >= 0) highlightCol = idx;
  }
  return highlightCol !== undefined ? { kind: "table", headers, rows, highlightCol } : { kind: "table", headers, rows };
}

function normBars(o: Obj): Block | null {
  const items: BarItem[] = [];
  const push = (labelRaw: unknown, valueRaw: unknown, hl: unknown) => {
    if (items.length >= CARD_LIMITS.maxBars) return;
    const label = clampStr(labelRaw, CARD_LIMITS.maxCellText);
    const value = typeof valueRaw === "boolean" ? null : parseNumber(valueRaw);
    if (!label || value === null) return;
    items.push(toBool(hl) ? { label, value, highlight: true } : { label, value });
  };
  const src = pickArr(o, ["items", "bars", "data", "points"]);
  if (src.length > 0) {
    for (const it of src) {
      if (isObj(it)) push(pickRaw(it, ["label", "name", "key", "x", "title"]), pickRaw(it, ["value", "val", "y", "count", "amount"]), it.highlight ?? it.accent);
      else if (Array.isArray(it)) push(it[0], it[1], it[2]);
    }
  } else {
    const labels = pickArr(o, ["labels", "xLabels", "x_labels", "categories"]);
    const values = asArr(o.values);
    if (labels.length > 0 && values.length > 0) labels.forEach((l, i) => push(l, values[i], false));
    else for (const [k, v] of mapEntries(o.data ?? o.values ?? o.items)) push(k, v, false);
  }
  if (items.length === 0) return null;
  const title = optPick(o, ["title", "label", "caption"], CARD_LIMITS.maxCellText);
  const unit = optPick(o, ["unit", "units", "suffix"], 24);
  return { kind: "bars", items, ...(title && { title }), ...(unit && { unit }) };
}

function numbers(src: unknown[], labelsOut?: string[]): number[] {
  const out: number[] = [];
  for (const v of src) {
    if (out.length >= CARD_LIMITS.maxLinePoints) break;
    if (isObj(v)) {
      const n = parseNumber(pickRaw(v, ["y", "value", "val"]));
      if (n === null) continue;
      labelsOut?.push(clampStr(pickRaw(v, ["x", "label", "name"]), 24) || String(out.length + 1));
      out.push(n);
    } else if (Array.isArray(v)) {
      const n = parseNumber(v[1]);
      if (n === null) continue;
      labelsOut?.push(clampStr(v[0], 24) || String(out.length + 1));
      out.push(n);
    } else {
      const n = typeof v === "boolean" ? null : parseNumber(v);
      if (n !== null) out.push(n);
    }
  }
  return out;
}

function normLine(o: Obj): Block | null {
  let xLabels = pickArr(o, ["xLabels", "x_labels", "labels", "categories", "x"])
    .slice(0, CARD_LIMITS.maxLinePoints)
    .map((l, i) => clampStr(l, 24) || String(i + 1));
  const series: LineSeries[] = [];
  const pointLabels: string[] = [];

  const rawSeries = pickArr(o, ["series", "datasets", "lines"]);
  if (rawSeries.length > 0) {
    for (const s of rawSeries) {
      if (series.length >= CARD_LIMITS.maxLineSeries) break;
      const labels: string[] = [];
      const values = isObj(s) ? numbers(pickArr(s, ["values", "points", "data", "y"]), labels) : numbers(asArr(s));
      if (values.length < 2) continue;
      if (pointLabels.length === 0) pointLabels.push(...labels);
      const name = isObj(s) ? optPick(s, ["name", "label", "title"], 48) : undefined;
      series.push(name ? { name, values } : { values });
    }
  } else {
    const values = numbers(pickArr(o, ["points", "values", "data", "y"]), pointLabels);
    if (values.length >= 2) series.push({ values });
  }
  if (series.length === 0) return null;
  if (xLabels.length === 0) xLabels = pointLabels;

  const maxLen = Math.max(...series.map((s) => s.values.length));
  xLabels = xLabels.slice(0, maxLen);
  while (xLabels.length < maxLen) xLabels.push(String(xLabels.length + 1));

  const title = optPick(o, ["title", "label", "caption"], CARD_LIMITS.maxCellText);
  const unit = optPick(o, ["unit", "units", "suffix"], 24);
  return { kind: "line", xLabels, series, ...(title && { title }), ...(unit && { unit }) };
}

function normTimeline(o: Obj): Block | null {
  const items: TimelineItem[] = [];
  for (const it of pickArr(o, ["items", "events", "timeline", "points"])) {
    if (items.length >= CARD_LIMITS.maxTimeline) break;
    if (!isObj(it)) continue;
    const date = pick(it, ["date", "year", "time", "when", "period"], 40);
    let label = pick(it, ["label", "title", "event", "name", "text"], CARD_LIMITS.maxShortText);
    let note = optPick(it, ["note", "detail", "details", "description", "desc"], CARD_LIMITS.maxShortText);
    if (!label && note) [label, note] = [note, undefined];
    if (!date || !label) continue;
    items.push(note && note !== label ? { date, label, note } : { date, label });
  }
  return items.length > 0 ? { kind: "timeline", items } : null;
}

function flowTone(it: Obj): "accent" | "muted" | undefined {
  const t = key(clampStr(it.tone ?? it.style ?? it.variant, 16));
  if (["accent", "highlight", "primary", "key", "main"].includes(t) || toBool(it.highlight) || toBool(it.accent)) return "accent";
  if (["muted", "secondary", "dim", "optional", "faded"].includes(t)) return "muted";
  return undefined;
}

function flowDirection(v: unknown): FlowDirection | undefined {
  const d = key(clampStr(v, 16));
  if (["lr", "rl", "leftright", "lefttoright", "horizontal", "row", "x"].includes(d)) return "lr";
  if (["td", "tb", "bt", "topdown", "toptobottom", "topbottom", "vertical", "column", "y"].includes(d)) return "td";
  return undefined;
}

function normFlow(o: Obj): Block | null {
  const nodes: FlowNode[] = [];
  const ids = new Set<string>();
  const addNode = (id: string, label: string, note?: string, tone?: "accent" | "muted"): boolean => {
    if (!id || !label || ids.has(id) || nodes.length >= CARD_LIMITS.maxFlowNodes) return false;
    ids.add(id);
    nodes.push({ id, label, ...(note && note !== label && { note }), ...(tone && { tone }) });
    return true;
  };

  for (const n of pickArr(o, ["nodes", "steps", "items"])) {
    if (isObj(n)) {
      const label = pick(n, ["label", "title", "text", "name"], CARD_LIMITS.maxCellText);
      const id = clampStr(pickRaw(n, ["id", "key"]), 48) || clampStr(label, 48);
      addNode(id, label || clampStr(id, CARD_LIMITS.maxCellText), optPick(n, ["note", "sub", "detail", "description"], CARD_LIMITS.maxCellText), flowTone(n));
    } else {
      const label = clampStr(n, CARD_LIMITS.maxCellText);
      addNode(clampStr(label, 48), label);
    }
  }
  const declared = nodes.length > 0;

  /** Endpoint by id, then by label; creates nodes when none were declared. */
  const resolve = (raw: unknown): string | null => {
    const s = clampStr(raw, 48);
    if (!s) return null;
    if (ids.has(s)) return s;
    const byLabel = nodes.find((n) => key(n.label) === key(s) || key(n.id) === key(s));
    if (byLabel) return byLabel.id;
    if (!declared && addNode(s, clampStr(raw, CARD_LIMITS.maxCellText))) return s;
    return null;
  };

  const edges: FlowEdge[] = [];
  const seenEdges = new Set<string>();
  for (const e of pickArr(o, ["edges", "links", "connections", "arrows"])) {
    if (edges.length >= CARD_LIMITS.maxFlowEdges) break;
    let fromRaw: unknown;
    let toRaw: unknown;
    let labelRaw: unknown;
    if (isObj(e)) {
      fromRaw = pickRaw(e, ["from", "source", "start", "src"]);
      toRaw = pickRaw(e, ["to", "target", "end", "dst"]);
      labelRaw = pickRaw(e, ["label", "text", "note"]);
    } else if (Array.isArray(e)) {
      [fromRaw, toRaw, labelRaw] = e;
    } else if (typeof e === "string") {
      const [path = "", ...rest] = e.split(":");
      const parts = path.split(/\s*(?:-+>|=+>|→|⟶)\s*/);
      if (parts.length < 2) continue;
      [fromRaw, toRaw] = parts;
      labelRaw = rest.join(":");
    } else continue;
    const from = resolve(fromRaw);
    const to = resolve(toRaw);
    if (!from || !to) continue;
    const label = clampStr(labelRaw, 48);
    const k = `${from}\u0000${to}\u0000${label}`;
    if (seenEdges.has(k)) continue;
    seenEdges.add(k);
    edges.push(label ? { from, to, label } : { from, to });
  }
  if (nodes.length === 0) return null;
  // Nodes listed without any arrows read as a sequence.
  if (edges.length === 0) {
    for (let i = 1; i < nodes.length; i++) edges.push({ from: nodes[i - 1]!.id, to: nodes[i]!.id });
  }
  const direction = flowDirection(o.direction ?? o.layout ?? o.orientation);
  return { kind: "flow", nodes, edges, ...(direction && { direction }) };
}

function compareKind(v: unknown): CompareItemKind | undefined {
  const s = key(clampStr(v, 16));
  if (["pro", "pros", "plus", "+", "good", "yes", "advantage"].includes(s)) return "pro";
  if (["con", "cons", "minus", "-", "bad", "no", "disadvantage"].includes(s)) return "con";
  if (s === "neutral") return "neutral";
  return undefined;
}

/** Reads `+ fast` / `− slow` / `✓` / `✗` prefixes as the item kind. */
function compareText(raw: string): CompareItem {
  const m = /^([+✓✔]|[-−–✗✘×])\s*(.+)$/su.exec(raw);
  if (!m) return { text: raw };
  return { text: m[2]!, kind: /[+✓✔]/u.test(m[1]!) ? "pro" : "con" };
}

function normCompare(o: Obj): Block | null {
  const columns: CompareColumn[] = [];
  for (const col of pickArr(o, ["columns", "items", "options", "sides"])) {
    if (columns.length >= CARD_LIMITS.maxCompareCols) break;
    if (!isObj(col)) continue;
    const title = pick(col, ["title", "name", "label"], CARD_LIMITS.maxCellText);
    if (!title) continue;
    const items: CompareItem[] = [];
    const add = (raw: unknown, forced?: CompareItemKind) => {
      if (items.length >= CARD_LIMITS.maxCompareItems) return;
      if (isObj(raw)) {
        const text = pick(raw, ["text", "label", "title", "value"], CARD_LIMITS.maxShortText);
        if (!text) return;
        const kind = forced ?? compareKind(raw.kind ?? raw.type);
        items.push(kind ? { text, kind } : compareText(text));
      } else {
        const text = clampStr(raw, CARD_LIMITS.maxShortText);
        if (!text) return;
        if (forced) items.push({ text, kind: forced });
        else items.push(compareText(text));
      }
    };
    for (const it of pickArr(col, ["items", "points", "features"])) add(it);
    for (const it of pickArr(col, ["pros", "plus", "advantages"])) add(it, "pro");
    for (const it of pickArr(col, ["cons", "minus", "disadvantages"])) add(it, "con");
    if (items.length === 0) continue;
    const subtitle = optPick(col, ["subtitle", "sub", "note", "price"], CARD_LIMITS.maxCellText);
    const accent = toBool(col.accent ?? col.highlight ?? col.recommended ?? col.winner);
    columns.push({ title, items, ...(subtitle && { subtitle }), ...(accent && { accent: true }) });
  }
  return columns.length > 0 ? { kind: "compare", columns } : null;
}

function normMath(o: Obj): Block | null {
  const lines: MathLine[] = [];
  for (const it of pickArr(o, ["lines", "steps", "items"])) {
    if (lines.length >= CARD_LIMITS.maxMathLines) break;
    if (isObj(it)) {
      const expr = pick(it, ["expr", "expression", "formula", "math", "latex", "line", "text"], CARD_LIMITS.maxShortText);
      if (!expr) continue;
      const note = optPick(it, ["note", "comment", "explanation", "why", "label"], CARD_LIMITS.maxShortText);
      lines.push(note && note !== expr ? { expr, note } : { expr });
    } else {
      const expr = clampStr(it, CARD_LIMITS.maxShortText);
      if (expr) lines.push({ expr });
    }
  }
  if (lines.length === 0) {
    const expr = pick(o, ["expr", "formula", "expression"], CARD_LIMITS.maxShortText);
    if (expr) lines.push({ expr });
  }
  if (lines.length === 0) return null;
  const result = optPick(o, ["result", "answer"], CARD_LIMITS.maxShortText);
  return result ? { kind: "math", lines, result } : { kind: "math", lines };
}

function joinList(v: unknown, max: number): string {
  return Array.isArray(v) ? clampStr(v.map((x) => clampStr(x, max)).filter(Boolean).join("; "), max) : clampStr(v, max);
}

function normDefinition(o: Obj): Block | null {
  const term = pick(o, ["term", "word", "title", "name"], CARD_LIMITS.maxCellText);
  const meaning = pick(o, ["meaning", "definition", "text", "description", "value"], CARD_LIMITS.maxText);
  if (!term || !meaning) return null;
  const example = joinList(pickRaw(o, ["example", "examples", "usage"]), CARD_LIMITS.maxShortText);
  return example ? { kind: "definition", term, meaning, example } : { kind: "definition", term, meaning };
}

const LETTERS_LAT = "abcdef";
const LETTERS_CYR = "абвгде";

function normQuiz(o: Obj): Block | null {
  const question = pick(o, ["question", "prompt", "q", "text", "title"], CARD_LIMITS.maxShortText);
  const options: string[] = [];
  let flagged: number | null = null;
  for (const opt of pickArr(o, ["options", "choices", "answers", "variants"])) {
    if (options.length >= CARD_LIMITS.maxQuizOptions) break;
    const text = isObj(opt) ? pick(opt, ["text", "label", "title", "value"], CARD_LIMITS.maxShortText) : clampStr(opt, CARD_LIMITS.maxShortText);
    if (!text) continue;
    if (isObj(opt) && flagged === null && toBool(opt.correct ?? opt.isCorrect ?? opt.right)) flagged = options.length;
    options.push(text);
  }
  if (!question || options.length < 2) return null;

  let correct: number | null = flagged;
  const raw = pickRaw(o, ["correct", "correctIndex", "correct_index", "answerIndex", "answer_index", "answer", "right"]);
  if (correct === null && raw !== undefined && typeof raw !== "boolean") {
    const asText = typeof raw === "string" ? cleanText(raw) : typeof raw === "number" ? String(raw) : "";
    const lower = asText.toLowerCase();
    const byText = options.findIndex((opt) => opt.toLowerCase() === lower);
    const n = typeof raw === "number" ? raw : /^[-+]?\d+$/.test(asText) ? Number(asText) : null;
    const letter = Array.from(lower.replace(/[).:\s]+$/, ""));
    const byLetter = letter.length === 1 ? Math.max(LETTERS_LAT.indexOf(letter[0]!), LETTERS_CYR.indexOf(letter[0]!)) : -1;
    if (n !== null && Number.isInteger(n) && n >= 0 && n < options.length && (typeof raw === "number" || byText < 0)) correct = n;
    else if (byText >= 0) correct = byText;
    else if (n !== null && Number.isInteger(n)) correct = Math.max(0, Math.min(options.length - 1, n));
    else if (byLetter >= 0 && byLetter < options.length) correct = byLetter;
  }
  // Without a known answer the quiz would mark the wrong option right.
  if (correct === null) return null;
  const explanation = optPick(o, ["explanation", "explain", "why", "rationale", "note"], CARD_LIMITS.maxText);
  return explanation ? { kind: "quiz", question, options, correct, explanation } : { kind: "quiz", question, options, correct };
}

function calloutTone(v: string): CalloutTone {
  const t = key(v);
  if (["tip", "hint", "success", "good", "ok", "done"].includes(t)) return "tip";
  if (["warning", "warn", "caution", "danger", "alert", "important", "error"].includes(t)) return "warning";
  return "idea";
}

function normCallout(o: Obj, alias: string): Block | null {
  const text = pick(o, ["text", "body", "content", "message", "note", "value"], CARD_LIMITS.maxText);
  if (!text) return null;
  const tone = calloutTone(pick(o, ["tone", "variant", "level", "style"], 16) || alias);
  const title = optPick(o, ["title", "heading", "label"], CARD_LIMITS.maxCellText);
  return title ? { kind: "callout", tone, title, text } : { kind: "callout", tone, text };
}

function normKv(o: Obj): Block | null {
  const items: KvItem[] = [];
  const push = (k: unknown, v: unknown) => {
    if (items.length >= CARD_LIMITS.maxKvItems) return;
    const keyText = clampStr(k, CARD_LIMITS.maxCellText);
    const value = joinList(v, CARD_LIMITS.maxShortText);
    if (keyText && value) items.push({ key: keyText, value });
  };
  const src = pickArr(o, ["items", "pairs", "facts", "rows", "data"]);
  for (const it of src) {
    if (Array.isArray(it)) push(it[0], it[1]);
    else if (isObj(it)) {
      const k = pickRaw(it, ["key", "label", "name", "k", "title"]);
      if (k !== undefined) push(k, pickRaw(it, ["value", "val", "v", "text"]));
      else {
        const entries = mapEntries(it);
        if (entries.length === 1) push(entries[0]![0], entries[0]![1]);
      }
    }
  }
  if (src.length === 0) for (const [k, v] of mapEntries(o.items ?? o.facts ?? o.data ?? o.pairs)) push(k, v);
  return items.length > 0 ? { kind: "kv", items } : null;
}

function normBlock(raw: unknown): Block | null {
  if (typeof raw === "string") return normText({ text: raw });
  if (!isObj(raw)) return null;
  const resolved = resolveKind(raw);
  if (!resolved) return null;
  switch (resolved.kind) {
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
      return normCallout(raw, resolved.alias);
    case "kv":
      return normKv(raw);
  }
}

/** Validates and clamps raw model output into a renderable card, or null. */
export function normalizeCard(input: unknown): CardSpec | null {
  let raw = input;
  if (typeof raw === "string") {
    try {
      raw = JSON.parse(raw);
    } catch {
      return null;
    }
  }
  if (!isObj(raw)) return null;
  let rawBlocks = pickArr(raw, ["blocks", "sections", "content"]);
  if (rawBlocks.length === 0 && (raw.kind !== undefined || raw.type !== undefined)) rawBlocks = [raw];
  const blocks: Block[] = [];
  for (const b of rawBlocks.slice(0, MAX_RAW_BLOCKS)) {
    if (blocks.length >= CARD_LIMITS.maxBlocks) break;
    const normalized = normBlock(b);
    if (normalized) blocks.push(normalized);
  }
  if (blocks.length === 0) return null;
  const title = pick(raw, ["title", "heading", "name"], CARD_LIMITS.maxTitle) || FALLBACK_TITLE;
  const subtitle = optPick(raw, ["subtitle", "sub", "description"], CARD_LIMITS.maxSubtitle);
  return subtitle ? { title, subtitle, blocks } : { title, blocks };
}
