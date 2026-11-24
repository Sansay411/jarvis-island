// Data structures for JARVIS explanation cards (rendered both in the desktop
// overlay window and inline inside the workspace chat transcript).

export const CARD_LIMITS = {
  maxTitle: 120,
  maxSubtitle: 200,
  maxBlocks: 8,
  maxText: 1200,
  maxShortText: 240,
  maxCellText: 120,
  maxSteps: 10,
  maxTableCols: 6,
  maxTableRows: 12,
  maxBars: 10,
  maxLineSeries: 4,
  maxLinePoints: 16,
  maxTimeline: 10,
  maxFlowNodes: 12,
  maxFlowEdges: 20,
  maxCompareCols: 3,
  maxCompareItems: 8,
  maxMathLines: 10,
  maxQuizOptions: 6,
  maxKvItems: 12,
} as const;

export interface TextBlock {
  kind: "text";
  text: string;
}

export interface StepItem {
  title: string;
  note?: string;
}

export interface StepsBlock {
  kind: "steps";
  items: StepItem[];
}

export interface TableBlock {
  kind: "table";
  headers: string[];
  rows: string[][];
  highlightCol?: number;
}

export interface BarItem {
  label: string;
  value: number;
  highlight?: boolean;
}

export interface BarsBlock {
  kind: "bars";
  title?: string;
  unit?: string;
  items: BarItem[];
}

export interface LineSeries {
  name?: string;
  values: number[];
}

export interface LineBlock {
  kind: "line";
  title?: string;
  unit?: string;
  xLabels: string[];
  series: LineSeries[];
}

export interface TimelineItem {
  date: string;
  label: string;
  note?: string;
}

export interface TimelineBlock {
  kind: "timeline";
  items: TimelineItem[];
}

export type FlowTone = "default" | "accent" | "muted";
export type FlowDirection = "lr" | "td";

export interface FlowNode {
  id: string;
  label: string;
  note?: string;
  tone?: FlowTone;
}

export interface FlowEdge {
  from: string;
  to: string;
  label?: string;
}

export interface FlowBlock {
  kind: "flow";
  direction?: FlowDirection;
  nodes: FlowNode[];
  edges: FlowEdge[];
}

export type CompareItemKind = "pro" | "con" | "neutral";

export interface CompareItem {
  text: string;
  kind?: CompareItemKind;
}

export interface CompareColumn {
  title: string;
  subtitle?: string;
  accent?: boolean;
  items: CompareItem[];
}

export interface CompareBlock {
  kind: "compare";
  columns: CompareColumn[];
}

export interface MathLine {
  expr: string;
  note?: string;
}

export interface MathBlock {
  kind: "math";
  lines: MathLine[];
  result?: string;
}

export interface DefinitionBlock {
  kind: "definition";
  term: string;
  meaning: string;
  example?: string;
}

export interface QuizBlock {
  kind: "quiz";
  question: string;
  options: string[];
  correct: number;
  explanation?: string;
}

export type CalloutTone = "idea" | "tip" | "warning";

export interface CalloutBlock {
  kind: "callout";
  tone?: CalloutTone;
  title?: string;
  text: string;
}

export interface KvItem {
  key: string;
  value: string;
}

export interface KvBlock {
  kind: "kv";
  items: KvItem[];
}

export type Block =
  | TextBlock
  | StepsBlock
  | TableBlock
  | BarsBlock
  | LineBlock
  | TimelineBlock
  | FlowBlock
  | CompareBlock
  | MathBlock
  | DefinitionBlock
  | QuizBlock
  | CalloutBlock
  | KvBlock;

export interface CardSpec {
  title: string;
  subtitle?: string;
  blocks: Block[];
}
