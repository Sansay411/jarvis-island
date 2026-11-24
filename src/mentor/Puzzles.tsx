// Brain games for «Перезагрузка»: sudoku (4×4 and 6×6 with JARVIS's clay
// icons instead of digits, classic 9×9 in three levels), the Schulte table
// and memory pairs. Every puzzle is generated here and has one solution.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ART } from "./art";

const ICONS = [ART.step, ART.streak, ART.trophy, ART.level, ART.feed, ART.words, ART.games, ART.breath, ART.sprint, ART.workout];

function shuffle<T>(a: T[]): T[] {
  const b = [...a];
  for (let i = b.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [b[i], b[j]] = [b[j]!, b[i]!];
  }
  return b;
}

function useClock(running: boolean) {
  const [sec, setSec] = useState(0);
  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => setSec((s) => s + 1), 1000);
    return () => window.clearInterval(id);
  }, [running]);
  return sec;
}

const clock = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

/* ---------------- sudoku ---------------- */

export interface SudokuMode {
  id: string;
  n: 4 | 6 | 9;
  /** box rows × box cols */
  box: [number, number];
  clues: number;
  icons: boolean;
  label: string;
  meta: string;
  base: number;
}

export const SUDOKU_MODES: SudokuMode[] = [
  { id: "mini", n: 4, box: [2, 2], clues: 6, icons: true, label: "Мини 4×4", meta: "картинки · 1 минута", base: 40 },
  { id: "six", n: 6, box: [2, 3], clues: 14, icons: true, label: "6×6", meta: "картинки · 3 минуты", base: 90 },
  { id: "easy", n: 9, box: [3, 3], clues: 40, icons: false, label: "9×9 лёгкая", meta: "цифры · 6 минут", base: 160 },
  { id: "mid", n: 9, box: [3, 3], clues: 32, icons: false, label: "9×9 средняя", meta: "цифры · 10 минут", base: 240 },
  { id: "hard", n: 9, box: [3, 3], clues: 26, icons: false, label: "9×9 сложная", meta: "для настоящих", base: 340 },
];

/** Cells sharing a row, column or box with `i`. */
function peersOf(n: number, box: [number, number]) {
  const all: number[][] = [];
  for (let i = 0; i < n * n; i++) {
    const r = Math.floor(i / n), c = i % n;
    const br = Math.floor(r / box[0]) * box[0], bc = Math.floor(c / box[1]) * box[1];
    const set = new Set<number>();
    for (let k = 0; k < n; k++) {
      set.add(r * n + k);
      set.add(k * n + c);
    }
    for (let y = br; y < br + box[0]; y++) for (let x = bc; x < bc + box[1]; x++) set.add(y * n + x);
    set.delete(i);
    all.push([...set]);
  }
  return all;
}

/** Count solutions of `g` (0 = empty), stopping at `limit`. */
function solve(g: number[], n: number, peers: number[][], limit: number, randomize: boolean): number {
  let best = -1, bestCount = n + 1;
  for (let i = 0; i < g.length; i++) {
    if (g[i]) continue;
    let count = 0;
    for (let v = 1; v <= n; v++) if (!peers[i]!.some((p) => g[p] === v)) count++;
    if (count < bestCount) {
      best = i;
      bestCount = count;
      if (count <= 1) break;
    }
  }
  if (best < 0) return 1;
  let found = 0;
  const values = Array.from({ length: n }, (_, k) => k + 1);
  for (const v of randomize ? shuffle(values) : values) {
    if (peers[best]!.some((p) => g[p] === v)) continue;
    g[best] = v;
    found += solve(g, n, peers, limit - found, randomize);
    if (found >= limit) {
      if (!randomize) g[best] = 0;
      return found;
    }
    g[best] = 0;
  }
  return found;
}

export function makeSudoku(mode: SudokuMode): { puzzle: number[]; solution: number[] } {
  const { n, box } = mode;
  const peers = peersOf(n, box);
  const solution = new Array<number>(n * n).fill(0);
  solve(solution, n, peers, 1, true);
  const puzzle = [...solution];
  let filled = n * n;
  for (const i of shuffle(Array.from({ length: n * n }, (_, k) => k))) {
    if (filled <= mode.clues) break;
    const keep = puzzle[i]!;
    puzzle[i] = 0;
    if (solve([...puzzle], n, peers, 2, false) !== 1) puzzle[i] = keep;
    else filled--;
  }
  return { puzzle, solution };
}

export function Sudoku({ onDone }: { onDone: (score: number, note: string) => void }) {
  const [mode, setMode] = useState<SudokuMode | null>(null);
  if (!mode) {
    return (
      <div className="pz-modes">
        <h3 className="pz-title">Судоку</h3>
        <p className="rs-hint">Каждая строка, столбец и блок без повторов. В мини-режимах вместо цифр мои 3D-иконки.</p>
        <div className="pz-mode-grid">
          {SUDOKU_MODES.map((m) => (
            <button key={m.id} type="button" className={`pz-mode${m.icons ? " is-icons" : ""}`} onClick={() => setMode(m)}>
              {m.icons ? (
                <span className="pz-mode-icons">
                  {ICONS.slice(0, m.n === 4 ? 4 : 3).map((src, k) => (
                    <img key={k} src={src} alt="" draggable={false} />
                  ))}
                </span>
              ) : (
                <span className="pz-mode-digits">{m.id === "hard" ? "7 · 3 · ?" : m.id === "mid" ? "5 · 9 · 2" : "1 · 2 · 3"}</span>
              )}
              <b>{m.label}</b>
              <span>{m.meta}</span>
            </button>
          ))}
        </div>
      </div>
    );
  }
  return <SudokuBoard key={mode.id} mode={mode} onDone={onDone} onBack={() => setMode(null)} />;
}

function SudokuBoard({ mode, onDone, onBack }: { mode: SudokuMode; onDone: (score: number, note: string) => void; onBack: () => void }) {
  const { n, box } = mode;
  const game = useMemo(() => makeSudoku(mode), [mode]);
  const peers = useMemo(() => peersOf(n, box), [n, box]);
  const icons = useMemo(() => (mode.icons ? shuffle(ICONS).slice(0, n) : []), [mode.icons, n]);
  const [grid, setGrid] = useState<number[]>(game.puzzle);
  const [notes, setNotes] = useState<number[][]>(() => game.puzzle.map(() => []));
  const [sel, setSel] = useState<number>(() => game.puzzle.findIndex((v) => v === 0));
  const [pencil, setPencil] = useState(false);
  const [mistakes, setMistakes] = useState(0);
  const [hints, setHints] = useState(0);
  const [wrong, setWrong] = useState<number | null>(null);
  const [won, setWon] = useState(false);
  const sec = useClock(!won);
  const done = useRef(false);

  const finish = useCallback(() => {
    if (done.current) return;
    done.current = true;
    setWon(true);
    const score = Math.max(10, mode.base - Math.floor(sec / 6) - mistakes * 12 - hints * 18);
    window.setTimeout(() => onDone(score, `${mode.label} за ${clock(sec)}${mistakes ? `, ошибок ${mistakes}` : ""}${hints ? `, подсказок ${hints}` : ""}`), 1300);
  }, [mode, sec, mistakes, hints, onDone]);

  const put = useCallback(
    (v: number) => {
      if (won || sel < 0 || game.puzzle[sel] !== 0) return;
      if (pencil && v > 0) {
        setNotes((ns) => ns.map((x, k) => (k === sel ? (x.includes(v) ? x.filter((y) => y !== v) : [...x, v].sort()) : x)));
        return;
      }
      if (v !== 0 && v !== game.solution[sel]) {
        setMistakes((m) => m + 1);
        setWrong(sel);
        window.setTimeout(() => setWrong(null), 600);
        return;
      }
      const next = grid.map((x, k) => (k === sel ? v : x));
      setGrid(next);
      if (v) setNotes((ns) => ns.map((x, k) => (k === sel ? [] : peers[sel]!.includes(k) ? x.filter((y) => y !== v) : x)));
      if (next.every((x, k) => x === game.solution[k])) finish();
      else if (v) {
        // Jump to the next empty cell: no mouse trips between moves.
        const after = next.findIndex((x, k) => k > sel && x === 0);
        setSel(after >= 0 ? after : next.findIndex((x) => x === 0));
      }
    },
    [won, sel, game, pencil, grid, peers, finish],
  );

  const hint = () => {
    const target = sel >= 0 && grid[sel] === 0 ? sel : grid.findIndex((x) => x === 0);
    if (target < 0) return;
    setHints((h) => h + 1);
    setSel(target);
    const next = grid.map((x, k) => (k === target ? game.solution[k]! : x));
    setGrid(next);
    if (next.every((x, k) => x === game.solution[k])) finish();
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key >= "1" && e.key <= String(n)) put(Number(e.key));
      else if (e.key === "Backspace" || e.key === "Delete" || e.key === "0") put(0);
      else if (e.key.toLowerCase() === "n" || e.key.toLowerCase() === "т") setPencil((p) => !p);
      else if (e.key.startsWith("Arrow")) {
        e.preventDefault();
        const d = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -n, ArrowDown: n }[e.key] ?? 0;
        setSel((s) => Math.min(n * n - 1, Math.max(0, (s < 0 ? 0 : s) + d)));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [n, put]);

  const selVal = sel >= 0 ? grid[sel] : 0;
  const face = (v: number) => (mode.icons ? <img src={icons[v - 1]} alt={String(v)} draggable={false} /> : v);
  const counts = Array.from({ length: n }, (_, k) => grid.filter((x) => x === k + 1).length);

  return (
    <div className={`pz-sudoku is-n${n}${won ? " is-won" : ""}`}>
      <div className="pz-bar">
        <button type="button" className="pz-back" onClick={onBack}>
          ← Режимы
        </button>
        <span className="pz-stat">{mode.label}</span>
        <span className="pz-stat">⏱ {clock(sec)}</span>
        <span className={`pz-stat${mistakes ? " is-bad" : ""}`}>Ошибки {mistakes}</span>
      </div>
      <div className="pz-grid" style={{ ["--n" as string]: n }} role="grid">
        {grid.map((v, i) => {
          const r = Math.floor(i / n), c = i % n;
          const cls = [
            "pz-cell",
            game.puzzle[i] ? "is-given" : "",
            i === sel ? "is-sel" : "",
            sel >= 0 && peers[sel]!.includes(i) ? "is-peer" : "",
            v && v === selVal ? "is-same" : "",
            wrong === i ? "is-wrong" : "",
            (c + 1) % box[1] === 0 && c < n - 1 ? "edge-r" : "",
            (r + 1) % box[0] === 0 && r < n - 1 ? "edge-b" : "",
          ].join(" ");
          return (
            <button key={i} type="button" className={cls} style={{ ["--d" as string]: `${(r + c) * 30}ms` }} onClick={() => setSel(i)}>
              {v ? (
                face(v)
              ) : notes[i]!.length ? (
                <span className="pz-notes" style={{ ["--b" as string]: box[1] }}>
                  {Array.from({ length: n }, (_, k) => (
                    <i key={k}>{notes[i]!.includes(k + 1) ? (mode.icons ? <img src={icons[k]} alt="" /> : k + 1) : ""}</i>
                  ))}
                </span>
              ) : null}
            </button>
          );
        })}
      </div>
      <div className="pz-pad">
        {Array.from({ length: n }, (_, k) => (
          <button key={k} type="button" className={`pz-key${counts[k]! >= n ? " is-full" : ""}`} onClick={() => put(k + 1)} disabled={won}>
            {face(k + 1)}
            {!mode.icons && <small>{n - counts[k]!}</small>}
          </button>
        ))}
      </div>
      <div className="pz-tools">
        <button type="button" className={`pz-tool${pencil ? " is-on" : ""}`} onClick={() => setPencil((p) => !p)}>
          ✎ Заметки {pencil ? "вкл" : "выкл"}
        </button>
        <button type="button" className="pz-tool" onClick={() => put(0)}>
          ⌫ Стереть
        </button>
        <button type="button" className="pz-tool is-hint" onClick={hint} disabled={won}>
          ✦ Подсказка Джарвиса
        </button>
      </div>
      {won && <div className="pz-win">Решено! {clock(sec)}</div>}
    </div>
  );
}

/* ---------------- Schulte table ---------------- */

export function Schulte({ onDone }: { onDone: (score: number, note: string) => void }) {
  const [cells, setCells] = useState(() => shuffle(Array.from({ length: 25 }, (_, k) => k + 1)));
  const [next, setNext] = useState(1);
  const [bad, setBad] = useState<number | null>(null);
  const [started, setStarted] = useState(false);
  const sec = useClock(started && next <= 25);
  const t0 = useRef(0);
  const tap = (v: number) => {
    if (!started) {
      setStarted(true);
      t0.current = performance.now();
    }
    if (v !== next) {
      setBad(v);
      window.setTimeout(() => setBad(null), 300);
      return;
    }
    if (v === 25) {
      const s = (performance.now() - t0.current) / 1000;
      setNext(26);
      window.setTimeout(() => onDone(Math.max(5, Math.round(180 - s * 3)), `25 чисел за ${s.toFixed(1)} с`), 900);
    } else setNext(v + 1);
  };
  return (
    <div className="pz-schulte">
      <div className="pz-bar">
        <span className="pz-stat">Найди: <b className="pz-next">{next <= 25 ? next : "✓"}</b></span>
        <span className="pz-stat">⏱ {sec} с</span>
        <button type="button" className="pz-back" onClick={() => (setCells(shuffle(cells)), setNext(1), setStarted(false))}>
          ↻ Заново
        </button>
      </div>
      <div className="pz-schulte-grid">
        {cells.map((v) => (
          <button key={v} type="button" className={`pz-sc${v < next ? " is-done" : ""}${bad === v ? " is-bad" : ""}`} onClick={() => tap(v)}>
            {v}
          </button>
        ))}
      </div>
      <p className="rs-hint">Смотри в центр и находи числа по порядку от 1 до 25. Тренирует внимание и периферийное зрение.</p>
    </div>
  );
}

/* ---------------- memory pairs ---------------- */

export function Memory({ onDone }: { onDone: (score: number, note: string) => void }) {
  const deck = useMemo(() => shuffle(shuffle(ICONS).slice(0, 8).flatMap((src, k) => [{ k, src }, { k, src }])), []);
  const [open, setOpen] = useState<number[]>([]);
  const [matched, setMatched] = useState<Set<number>>(new Set());
  const [moves, setMoves] = useState(0);
  const sec = useClock(matched.size < 8 && moves > 0);
  const flip = (i: number) => {
    if (open.length === 2 || open.includes(i) || matched.has(deck[i]!.k)) return;
    const now = [...open, i];
    setOpen(now);
    if (now.length === 2) {
      setMoves((m) => m + 1);
      const [a, b] = now as [number, number];
      if (deck[a]!.k === deck[b]!.k) {
        const m = new Set(matched).add(deck[a]!.k);
        setMatched(m);
        setOpen([]);
        if (m.size === 8) window.setTimeout(() => onDone(Math.max(10, 120 - (moves + 1 - 8) * 6 - Math.floor(sec / 3)), `${moves + 1} ходов, ${sec} с`), 900);
      } else window.setTimeout(() => setOpen([]), 750);
    }
  };
  return (
    <div className="pz-memory">
      <div className="pz-bar">
        <span className="pz-stat">Пары {matched.size} из 8</span>
        <span className="pz-stat">Ходы {moves}</span>
        <span className="pz-stat">⏱ {sec} с</span>
      </div>
      <div className="pz-mem-grid">
        {deck.map((c, i) => {
          const up = open.includes(i) || matched.has(c.k);
          return (
            <button key={i} type="button" className={`pz-mem${up ? " is-up" : ""}${matched.has(c.k) ? " is-matched" : ""}`} onClick={() => flip(i)}>
              <span className="pz-mem-inner">
                <span className="pz-mem-back">
                  <i />
                  <i />
                </span>
                <span className="pz-mem-face">
                  <img src={c.src} alt="" draggable={false} />
                </span>
              </span>
            </button>
          );
        })}
      </div>
      <p className="rs-hint">Открывай по две карты и запоминай, где что лежит. Меньше ходов, больше очков.</p>
    </div>
  );
}
