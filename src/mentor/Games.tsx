// Sixty-second games with a personal record: mental arithmetic and English
// words at speed. Fast feedback and «beat your best» — the same pull as a
// feed, spent on a skill.

import { useEffect, useRef, useState } from "react";
import { mentorIpc } from "../store/mentor";
import { ART, type ArtName } from "./art";

export type GameId = "sprint" | "words" | "sudoku" | "schulte" | "memory";
export const GAME_TITLE: Record<GameId, string> = {
  sprint: "Спринт счёта",
  words: "Слова на скорость",
  sudoku: "Судоку",
  schulte: "Таблица Шульте",
  memory: "Мемори",
};
/** The track a game trains. */
export const GAME_TRACK: Record<GameId, string> = { sprint: "code", words: "english", sudoku: "mind", schulte: "mind", memory: "mind" };
const GAMES: Array<{ id: GameId; art: ArtName; meta: string; text: string }> = [
  { id: "sudoku", art: "level", meta: "4×4 с картинками до 9×9", text: "Мини-судоку с моими иконками или классика трёх уровней. Подсказка, если застрял." },
  { id: "sprint", art: "sprint", meta: "60 секунд", text: "Примеры в уме на время. Каждый верный: очко, сложность растёт." },
  { id: "words", art: "words", meta: "60 секунд", text: "Слово и перевод. Чем быстрее, тем больше слов." },
  { id: "schulte", art: "streak", meta: "25 чисел", text: "Найди числа по порядку как можно быстрее. Внимание как у пилота." },
  { id: "memory", art: "feed", meta: "8 пар", text: "Открывай карты и запоминай. Меньше ходов, больше очков." },
];
/** Games with their own screen and finish rule (not the 60-second clock). */
export const PUZZLES: GameId[] = ["sudoku", "schulte", "memory"];
const LENGTH = 60;

export function GamePicker({ onPick }: { onPick: (g: GameId) => void }) {
  const [best, setBest] = useState<Partial<Record<GameId, number>>>({});
  useEffect(() => {
    void Promise.all(GAMES.map((g) => mentorIpc.best(GAME_TITLE[g.id]).then((v) => [g.id, v] as const))).then((all) => setBest(Object.fromEntries(all)));
  }, []);
  return (
    <div className="rs-games is-many">
      {GAMES.map((g, k) => (
        <button key={g.id} type="button" className={`rs-tile${k === 0 ? " is-feature" : ""}`} onClick={() => onPick(g.id)}>
          <img className="rs-tile-art" src={ART[g.art]} alt="" draggable={false} />
          {k === 0 && <span className="rs-tile-badge">Новое</span>}
          <b>{GAME_TITLE[g.id]}</b>
          <span className="rs-tile-meta">
            {g.meta}
            {best[g.id] ? ` · рекорд ${best[g.id]}` : ""}
          </span>
          <span className="rs-tile-text">{g.text}</span>
        </button>
      ))}
    </div>
  );
}

function problem(level: number): { text: string; answer: number } {
  const r = (a: number, b: number) => a + Math.floor(Math.random() * (b - a + 1));
  const kind = level < 4 ? r(0, 1) : r(0, 2);
  if (kind === 0) {
    const a = r(12, 20 + level * 8);
    const b = r(7, 15 + level * 6);
    return { text: `${a} + ${b}`, answer: a + b };
  }
  if (kind === 1) {
    const a = r(20, 40 + level * 8);
    const b = r(5, a - 3);
    return { text: `${a} − ${b}`, answer: a - b };
  }
  const a = r(3, 9);
  const b = r(4, 12 + level);
  return { text: `${a} × ${b}`, answer: a * b };
}

export function Game({ game, onDone }: { game: GameId; onDone: (score: number) => void }) {
  const [left, setLeft] = useState(LENGTH);
  const [score, setScore] = useState(0);
  const [flash, setFlash] = useState<"ok" | "bad" | null>(null);
  const [best, setBest] = useState(0);
  const finished = useRef(false);

  useEffect(() => {
    void mentorIpc.best(GAME_TITLE[game]).then(setBest);
  }, [game]);
  useEffect(() => {
    if (left <= 0) {
      if (!finished.current) {
        finished.current = true;
        onDone(score);
      }
      return;
    }
    const id = window.setTimeout(() => setLeft((l) => l - 1), 1000);
    return () => window.clearTimeout(id);
  }, [left, score, onDone]);

  const hit = (ok: boolean) => {
    setFlash(ok ? "ok" : "bad");
    window.setTimeout(() => setFlash(null), 220);
    if (ok) setScore((s) => s + 1);
  };

  return (
    <div className={`rs-game${flash ? ` is-${flash}` : ""}`}>
      <div className="rs-game-top">
        <span className="rs-game-time" style={{ ["--p" as string]: `${(left / LENGTH) * 100}%` }}>
          {left} с
        </span>
        <span className="rs-game-score">
          {score}
          <small>рекорд {Math.max(best, score)}</small>
        </span>
      </div>
      {game === "sprint" ? <Sprint level={Math.floor(score / 5)} onAnswer={hit} /> : <Words onAnswer={hit} />}
    </div>
  );
}

function Sprint({ level, onAnswer }: { level: number; onAnswer: (ok: boolean) => void }) {
  const [p, setP] = useState(() => problem(level));
  const [value, setValue] = useState("");
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => ref.current?.focus(), [p]);
  return (
    <form
      className="rs-sprint"
      onSubmit={(e) => {
        e.preventDefault();
        if (!value.trim()) return;
        onAnswer(Number(value) === p.answer);
        setValue("");
        setP(problem(level));
      }}
    >
      <div className="rs-sprint-q">{p.text} =</div>
      <input ref={ref} className="rs-sprint-input" inputMode="numeric" value={value} onChange={(e) => setValue(e.target.value.replace(/[^\d-]/g, ""))} aria-label="Ответ" autoFocus />
      <span className="rs-hint">Ответ и Enter</span>
    </form>
  );
}

function Words({ onAnswer }: { onAnswer: (ok: boolean) => void }) {
  const [pairs, setPairs] = useState<Array<[string, string]>>([]);
  const [k, setK] = useState(0);
  useEffect(() => {
    void mentorIpc.words().then(setPairs);
  }, []);
  const [opts, setOpts] = useState<string[]>([]);
  useEffect(() => {
    if (pairs.length < 4) return;
    const right = pairs[k % pairs.length]![1];
    const others = pairs.filter((_, j) => j !== k % pairs.length).sort(() => Math.random() - 0.5).slice(0, 3).map((x) => x[1]);
    setOpts([right, ...others].sort(() => Math.random() - 0.5));
  }, [pairs, k]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const n = Number(e.key);
      if (n >= 1 && n <= 4 && opts[n - 1]) pick(opts[n - 1]!);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  if (pairs.length < 4) return <div className="rs-loading">Загружаю слова…</div>;
  const [word, tr] = pairs[k % pairs.length]!;
  function pick(o: string) {
    onAnswer(o === tr);
    setK((x) => x + 1);
  }
  return (
    <div className="rs-words">
      <div className="rs-word">{word}</div>
      <div className="rs-options">
        {opts.map((o, j) => (
          <button key={o} type="button" className="rs-option" onClick={() => pick(o)}>
            <kbd>{j + 1}</kbd>
            <span>{o}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
