// Interactive explanation card rendered both in the desktop overlay window
// and inline inside the workspace chat transcript.

import { memo, useMemo, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { Icon } from "../../ui/Icon";
import { Markdown } from "../../workspace/Markdown";
import { BarsChart, FlowDiagram, LineChart } from "./Charts";
import { prettyMath } from "./math";
import { normalizeCard } from "./normalize";
import type {
  Block,
  CalloutBlock,
  CardSpec,
  CompareBlock,
  DefinitionBlock,
  KvBlock,
  MathBlock,
  QuizBlock,
  StepsBlock,
  TableBlock,
  TimelineBlock,
} from "./types";
import "./cards.css";

export interface CardViewProps {
  /** Rendered as given after a defensive `normalizeCard` pass (idempotent). */
  spec: CardSpec;
  onClose?: () => void;
  /** Tighter surface for inline rendering inside the chat transcript. */
  compact?: boolean;
}

const QUIZ_LETTERS = ["А", "Б", "В", "Г", "Д", "Е"];
const EASE_OUT: [number, number, number, number] = [0.22, 1, 0.36, 1];

/** Miniature JARVIS two-pill amber eyes mark for the card header. */
function JarvisMark() {
  return (
    <svg className="jcard-mark" width={16} height={14} viewBox="0 0 16 14" aria-hidden="true">
      <rect x={3} y={2.5} width={3.2} height={9} rx={1.6} fill="var(--amber)" />
      <rect x={9.8} y={2.5} width={3.2} height={9} rx={1.6} fill="var(--amber)" />
    </svg>
  );
}

function StepsView({ block }: { block: StepsBlock }) {
  return (
    <ol className="jcard-steps">
      {block.items.map((step, i) => (
        <li key={i} className="jcard-step">
          <span className="jcard-step-num" aria-hidden="true">
            {i + 1}
          </span>
          <div className="jcard-step-body">
            <div className="jcard-step-title">{step.title}</div>
            {step.note && <div className="jcard-step-note">{step.note}</div>}
          </div>
        </li>
      ))}
    </ol>
  );
}

function TableView({ block }: { block: TableBlock }) {
  return (
    <div className="jcard-table-wrap">
      <table className="jcard-table">
        {block.headers.some(Boolean) && (
          <thead>
            <tr>
              {block.headers.map((h, ci) => (
                <th key={ci} className={block.highlightCol === ci ? "is-hl" : undefined}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
        )}
        <tbody>
          {block.rows.map((row, ri) => (
            <tr key={ri}>
              {row.map((cell, ci) => (
                <td key={ci} className={block.highlightCol === ci ? "is-hl" : undefined}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TimelineView({ block }: { block: TimelineBlock }) {
  return (
    <ol className="jcard-timeline">
      {block.items.map((item, i) => (
        <li key={i} className="jcard-tl-item">
          <span className="jcard-tl-dot" aria-hidden="true" />
          <div className="jcard-tl-body">
            <div className="jcard-tl-top">
              <span className="jcard-tl-date">{item.date}</span>
              <span className="jcard-tl-label">{item.label}</span>
            </div>
            {item.note && <div className="jcard-tl-note">{item.note}</div>}
          </div>
        </li>
      ))}
    </ol>
  );
}

function CompareView({ block }: { block: CompareBlock }) {
  const cols = block.columns.length;
  return (
    <div className={`jcard-compare cols-${cols}`}>
      {block.columns.map((col, ci) => (
        <div key={ci} className={`jcard-cmp-col${col.accent ? " is-accent" : ""}`}>
          <div className="jcard-cmp-head">
            <div className="jcard-cmp-title">{col.title}</div>
            {col.subtitle && <div className="jcard-cmp-sub">{col.subtitle}</div>}
          </div>
          <ul className="jcard-cmp-list">
            {col.items.map((it, ii) => {
              const kind = it.kind ?? "neutral";
              const glyph = kind === "pro" ? "+" : kind === "con" ? "−" : "•";
              return (
                <li key={ii} className={`jcard-cmp-item is-${kind}`}>
                  <span className="jcard-cmp-glyph" aria-hidden="true">
                    {glyph}
                  </span>
                  <span>{it.text}</span>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </div>
  );
}

function MathView({ block }: { block: MathBlock }) {
  return (
    <div className="jcard-math">
      <div className="jcard-math-lines">
        {block.lines.map((line, i) => (
          <div key={i} className="jcard-math-line">
            <code className="jcard-math-expr">{prettyMath(line.expr)}</code>
            {line.note && <span className="jcard-math-note">{line.note}</span>}
          </div>
        ))}
      </div>
      {block.result && (
        <div className="jcard-math-result">
          <span className="jcard-math-result-label">Ответ</span>
          <code className="jcard-math-result-val">{prettyMath(block.result)}</code>
        </div>
      )}
    </div>
  );
}

function DefinitionView({ block }: { block: DefinitionBlock }) {
  return (
    <div className="jcard-def">
      <div className="jcard-def-term">{block.term}</div>
      <div className="jcard-def-meaning">{block.meaning}</div>
      {block.example && (
        <div className="jcard-def-example">
          <span className="jcard-def-example-tag">Пример</span>
          <span>{block.example}</span>
        </div>
      )}
    </div>
  );
}

function QuizView({ block }: { block: QuizBlock }) {
  const [picked, setPicked] = useState<number | null>(null);
  const reduced = useReducedMotion();
  const answered = picked !== null;
  const isRight = picked === block.correct;

  return (
    <div className="jcard-quiz">
      <div className="jcard-quiz-head">
        <span className="jcard-quiz-badge">Проверка</span>
        <div className="jcard-quiz-q">{block.question}</div>
      </div>
      <div className="jcard-quiz-options" role="group" aria-label={block.question}>
        {block.options.map((opt, idx) => {
          const selected = picked === idx;
          const correct = idx === block.correct;
          let cls = "jcard-quiz-opt";
          if (answered) {
            if (correct) cls += " is-correct";
            else if (selected) cls += " is-wrong";
            else cls += " is-dim";
          }
          return (
            <button
              key={idx}
              type="button"
              className={cls}
              disabled={answered}
              aria-pressed={selected}
              onClick={() => setPicked(idx)}
            >
              <span className="jcard-quiz-key" aria-hidden="true">
                {QUIZ_LETTERS[idx] ?? String(idx + 1)}
              </span>
              <span className="jcard-quiz-text">{opt}</span>
              {answered && correct && <Icon name="check" size={14} className="jcard-quiz-icon is-ok" />}
              {answered && selected && !correct && <Icon name="close" size={13} className="jcard-quiz-icon is-bad" />}
            </button>
          );
        })}
      </div>
      {answered && (
        <motion.div
          className={`jcard-quiz-feedback ${isRight ? "is-ok" : "is-bad"}`}
          role="status"
          initial={reduced ? false : { opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={reduced ? { duration: 0 } : { duration: 0.18, ease: EASE_OUT }}
        >
          <div className="jcard-quiz-verdict">
            {isRight ? "Верно" : `Не совсем — верный вариант: ${QUIZ_LETTERS[block.correct] ?? block.correct + 1}`}
          </div>
          {block.explanation && <div className="jcard-quiz-exp">{block.explanation}</div>}
        </motion.div>
      )}
    </div>
  );
}

function CalloutView({ block }: { block: CalloutBlock }) {
  const tone = block.tone ?? "idea";
  const iconName = tone === "warning" ? "alert" : tone === "tip" ? "check" : "spark";
  return (
    <div className={`jcard-callout is-${tone}`}>
      <span className="jcard-callout-icon" aria-hidden="true">
        <Icon name={iconName} size={14} />
      </span>
      <div className="jcard-callout-body">
        {block.title && <div className="jcard-callout-title">{block.title}</div>}
        <div className="jcard-callout-text">
          <Markdown text={block.text} />
        </div>
      </div>
    </div>
  );
}

function KvView({ block }: { block: KvBlock }) {
  return (
    <dl className="jcard-kv">
      {block.items.map((it, i) => (
        <div key={i} className="jcard-kv-row">
          <dt className="jcard-kv-key">{it.key}</dt>
          <dd className="jcard-kv-val">{it.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function BlockView({ block }: { block: Block }) {
  switch (block.kind) {
    case "text":
      return (
        <div className="jcard-text">
          <Markdown text={block.text} />
        </div>
      );
    case "steps":
      return <StepsView block={block} />;
    case "table":
      return <TableView block={block} />;
    case "bars":
      return <BarsChart block={block} />;
    case "line":
      return <LineChart block={block} />;
    case "timeline":
      return <TimelineView block={block} />;
    case "flow":
      return <FlowDiagram block={block} />;
    case "compare":
      return <CompareView block={block} />;
    case "math":
      return <MathView block={block} />;
    case "definition":
      return <DefinitionView block={block} />;
    case "quiz":
      // A new question starts unanswered.
      return <QuizView key={`${block.question}|${block.options.join("|")}`} block={block} />;
    case "callout":
      return <CalloutView block={block} />;
    case "kv":
      return <KvView block={block} />;
  }
}

export const CardView = memo(function CardView({ spec: input, onClose, compact = false }: CardViewProps) {
  const reduced = useReducedMotion();
  // The overlay may hand over whatever arrived on the event bus.
  const spec = useMemo(() => normalizeCard(input), [input]);
  if (!spec) return null;

  return (
    <motion.section
      className={`jcard${compact ? " is-compact" : ""}`}
      data-overlay-interactive=""
      aria-label={spec.title}
      initial={compact || reduced ? false : { opacity: 0, y: 8, scale: 0.985 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={compact || reduced ? { duration: 0 } : { duration: 0.22, ease: EASE_OUT }}
    >
      <header className="jcard-head">
        <div className="jcard-head-main">
          <JarvisMark />
          <div className="jcard-titles">
            <h3 className="jcard-title">{spec.title}</h3>
            {spec.subtitle && <p className="jcard-subtitle">{spec.subtitle}</p>}
          </div>
        </div>
        {onClose && (
          <button
            type="button"
            className="jcard-close"
            onClick={onClose}
            aria-label="Закрыть карточку"
            title="Закрыть"
          >
            <Icon name="close" size={14} />
          </button>
        )}
      </header>

      <div className="jcard-body scroll">
        {spec.blocks.map((block, idx) => (
          <div key={`${block.kind}-${idx}`} className="jcard-block">
            <BlockView block={block} />
          </div>
        ))}
      </div>
    </motion.section>
  );
});
