// The body options: a three-move workout (reps you confirm, holds that
// count down) and one minute of cyclic sighing — two quick inhales through
// the nose, one long exhale (Balban et al., Cell Reports Medicine 2023).

import { useEffect, useState } from "react";
import { mentorIpc, type WorkoutMove } from "../store/mentor";

export function Workout({ onDone }: { onDone: (moves: number) => void }) {
  const [moves, setMoves] = useState<WorkoutMove[] | null>(null);
  const [i, setI] = useState(0);
  const [left, setLeft] = useState<number | null>(null);
  const [done, setDone] = useState(0);
  useEffect(() => {
    void mentorIpc.workout().then(setMoves);
  }, []);
  const move = moves?.[i];
  const advance = (counted: boolean) => {
    const n = done + (counted ? 1 : 0);
    setDone(n);
    setLeft(null);
    if (!moves || i + 1 >= moves.length) onDone(n);
    else setI(i + 1);
  };
  useEffect(() => {
    if (left === null) return;
    if (left <= 0) {
      advance(true);
      return;
    }
    const id = window.setTimeout(() => setLeft(left - 1), 1000);
    return () => window.clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [left]);
  if (!moves || !move) return <div className="rs-loading">Готовлю разминку…</div>;
  const total = move.seconds ?? 0;
  return (
    <div className="rs-body-screen">
      <ol className="steps rs-moves">
        {moves.map((m, k) => (
          <li key={k} className={`step${k < i ? " is-done" : k === i ? " is-run" : ""}`}>
            <span className="mark" aria-hidden="true" />
            <span className="step-text">
              <b>{m.title}</b>
              <small>{m.reps ? `${m.reps} раз · ${m.hint}` : `${m.seconds} секунд · ${m.hint}`}</small>
            </span>
          </li>
        ))}
      </ol>
      <div className="rs-move">
        {move.reps ? (
          <>
            <div className="rs-move-big">{move.reps}</div>
            <div className="rs-move-unit">повторений</div>
            <button type="button" className="btn btn-primary" onClick={() => advance(true)}>
              Сделал
            </button>
          </>
        ) : (
          <>
            <div className="rs-ring" style={{ ["--p" as string]: left === null ? 1 : left / Math.max(1, total) }}>
              <span>{left ?? total}</span>
            </div>
            <div className="rs-move-unit">секунд</div>
            {left === null && (
              <button type="button" className="btn btn-primary" onClick={() => setLeft(total)}>
                Старт
              </button>
            )}
          </>
        )}
        <button type="button" className="rs-link" onClick={() => advance(false)}>
          Пропустить
        </button>
      </div>
    </div>
  );
}

const PHASES = [
  { label: "Вдох носом", ms: 2000, scale: 0.8 },
  { label: "Ещё короткий вдох", ms: 1000, scale: 1 },
  { label: "Долгий выдох ртом", ms: 5000, scale: 0.35 },
];
const CYCLES = 6;

export function Breath({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState(-1);
  useEffect(() => {
    if (step < 0) return;
    if (step >= CYCLES * PHASES.length) {
      onDone();
      return;
    }
    const id = window.setTimeout(() => setStep(step + 1), PHASES[step % PHASES.length]!.ms);
    return () => window.clearTimeout(id);
  }, [step, onDone]);
  const phase = step >= 0 ? PHASES[step % PHASES.length]! : null;
  return (
    <div className="rs-breath">
      <div className="rs-breath-orb" style={{ transform: `scale(${phase?.scale ?? 0.45})`, transitionDuration: `${phase?.ms ?? 600}ms` }} />
      <div className="rs-breath-text">
        {phase ? (
          <>
            <b>{phase.label}</b>
            <span>
              Цикл {Math.min(CYCLES, Math.floor(step / PHASES.length) + 1)} из {CYCLES}
            </span>
          </>
        ) : (
          <>
            <b>Циклический вздох</b>
            <span>Два вдоха носом, один длинный выдох. Минута — и пульс ниже, голова яснее.</span>
            <button type="button" className="btn btn-primary" onClick={() => setStep(0)}>
              Начать
            </button>
          </>
        )}
      </div>
    </div>
  );
}
