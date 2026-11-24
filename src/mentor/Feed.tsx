// «Лента знаний»: seven cards, one at a time, like a short-video feed —
// but each one is a question, a word, a line of code or a thirty-second
// exercise. Answers are instant, a streak of right answers multiplies XP,
// sometimes a rare card pays triple, and the set ends (no endless scroll).

import { useEffect, useMemo, useRef, useState } from "react";
import { mentorIpc, type FeedCard } from "../store/mentor";
import { Icon, type IconName } from "../ui/Icon";

const TRACK: Record<string, string> = { code: "Код", english: "English", mind: "Логика", body: "Тело", projects: "Проекты" };
const TRACK_ICON: Record<string, IconName> = { code: "code", english: "book", mind: "spark", body: "heart", projects: "rocket" };

export interface FeedResult {
  xp: number;
  right: number;
  answered: number;
  bestCombo: number;
  minutes: number;
}

export function Feed({ onDone }: { onDone: (r: FeedResult) => void }) {
  const [cards, setCards] = useState<FeedCard[] | null>(null);
  const [i, setI] = useState(0);
  const [picked, setPicked] = useState<number | null>(null);
  const [doneCard, setDoneCard] = useState(false);
  const [xp, setXp] = useState(0);
  const [gain, setGain] = useState<{ v: number; k: number } | null>(null);
  const [combo, setCombo] = useState(0);
  const [stats, setStats] = useState({ right: 0, answered: 0, bestCombo: 0 });
  const shownAt = useRef(performance.now());
  const started = useRef(performance.now());

  useEffect(() => {
    void mentorIpc.feed(7).then(setCards);
    // While this set is played, JARVIS writes the next ones (around the
    // last mistakes); the backend keeps it to one batch in ten minutes.
    void mentorIpc.generate().catch(() => {});
  }, []);
  useEffect(() => {
    shownAt.current = performance.now();
  }, [i]);

  const card = cards?.[i];
  const answer = typeof card?.data.answer === "number" ? (card.data.answer as number) : -1;
  const options = (card?.data.options as string[] | undefined) ?? [];
  const isQuestion = card ? ["quiz", "code", "word"].includes(card.kind) : false;
  const settled = isQuestion ? picked !== null : doneCard;

  const award = (v: number) => {
    setXp((x) => x + v);
    setGain({ v, k: Date.now() });
  };

  const choose = (k: number) => {
    if (!card || picked !== null) return;
    setPicked(k);
    const right = k === answer;
    const quick = performance.now() - shownAt.current < 6000;
    const nextCombo = right ? combo + 1 : 0;
    const mult = (1 + Math.min(4, combo) * 0.25) * (card.rare ? 3 : 1);
    const v = right ? Math.round(10 * mult) : 3;
    setCombo(nextCombo);
    setStats((s) => ({ right: s.right + (right ? 1 : 0), answered: s.answered + 1, bestCombo: Math.max(s.bestCombo, nextCombo) }));
    award(v);
    void mentorIpc.answer(card.id, right, quick, v);
  };

  const finishCard = (v: number) => {
    if (!card || doneCard) return;
    setDoneCard(true);
    award(card.rare ? v * 3 : v);
    void mentorIpc.answer(card.id, true, true, card.rare ? v * 3 : v);
  };

  const next = () => {
    if (!cards) return;
    if (i + 1 >= cards.length) {
      onDone({ xp, right: stats.right, answered: stats.answered, bestCombo: stats.bestCombo, minutes: (performance.now() - started.current) / 60_000 });
      return;
    }
    setI(i + 1);
    setPicked(null);
    setDoneCard(false);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (isQuestion && picked === null && ["1", "2", "3", "4"].includes(e.key)) {
        choose(Number(e.key) - 1);
        e.preventDefault();
      } else if (settled && (e.key === "Enter" || e.key === " " || e.key === "ArrowDown")) {
        next();
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const wheelLock = useRef(0);
  const onWheel = (e: React.WheelEvent) => {
    if (!settled || e.deltaY < 30) return;
    const now = performance.now();
    if (now - wheelLock.current < 600) return;
    wheelLock.current = now;
    next();
  };

  const dots = useMemo(() => cards?.map((_, k) => (k < i ? "is-done" : k === i ? "is-on" : "")) ?? [], [cards, i]);

  if (!cards) return <div className="rs-feed rs-loading">Собираю ленту…</div>;
  if (!card) return null;
  const d = card.data as Record<string, string>;

  return (
    <div className="rs-feed" onWheel={onWheel}>
      <div className="rs-feed-top">
        <div className="rs-dots" aria-label={`Карточка ${i + 1} из ${cards.length}`}>
          {dots.map((c, k) => (
            <i key={k} className={c} />
          ))}
        </div>
        <div className="rs-feed-meta">
          {combo >= 2 && (
            <span className="rs-combo">
              <Icon name="flame" size={13} />
              Серия ×{combo}
            </span>
          )}
          <span className="rs-xp">
            <Icon name="spark" size={15} />
            {xp} XP
            {gain && (
              <b key={gain.k} className="rs-gain">
                +{gain.v}
              </b>
            )}
          </span>
        </div>
      </div>

      <article key={card.id} className={`rs-card${card.rare ? " is-rare" : ""}`}>
        <div className="rs-card-tags">
          <span className={`rs-tag is-${card.track}`}>
            <Icon name={TRACK_ICON[card.track] ?? "spark"} size={13} />
            {TRACK[card.track] ?? card.track}
          </span>
          {card.review && (
            <span className="rs-tag is-review">
              <Icon name="restart" size={13} />
              Повтор
            </span>
          )}
          {card.rare && (
            <span className="rs-tag is-rare">
              <Icon name="spark" size={13} />
              Редкая · ×3 XP
            </span>
          )}
        </div>

        {card.kind === "word" && (
          <>
            <div className="rs-word">{d.word}</div>
            {d.ipa && <div className="rs-ipa">[{d.ipa}]</div>}
            <div className="rs-q">Что это значит?</div>
          </>
        )}
        {card.kind === "quiz" && <div className="rs-q is-big">{d.q}</div>}
        {card.kind === "code" && (
          <>
            <div className="rs-q">{d.title}</div>
            <pre className="rs-code">{d.code}</pre>
          </>
        )}
        {(card.kind === "fact" || card.kind === "challenge") && (
          <>
            <div className="rs-q is-big">{d.title}</div>
            <p className="rs-body">{d.body}</p>
          </>
        )}

        {isQuestion && (
          <div className="rs-options">
            {options.map((o, k) => {
              const state = picked === null ? "" : k === answer ? " is-right" : k === picked ? " is-wrong" : " is-dim";
              return (
                <button key={k} type="button" className={`rs-option${state}`} onClick={() => choose(k)} disabled={picked !== null}>
                  <kbd>{k + 1}</kbd>
                  <span>{o}</span>
                </button>
              );
            })}
          </div>
        )}

        {isQuestion && picked !== null && (
          <div className={`rs-explain${picked === answer ? " is-right" : " is-wrong"}`}>
            <b>{picked === answer ? (combo >= 3 ? "Огонь — серия!" : "Верно") : "Мимо — запомним и повторим завтра"}</b>
            {card.kind === "word" ? (
              <span>
                {d.ex} — {d.exRu}
              </span>
            ) : (
              <span>{d.explain}</span>
            )}
          </div>
        )}

        {card.kind === "fact" && !doneCard && (
          <div className="rs-card-actions">
            <button type="button" className="btn btn-primary" onClick={() => finishCard(4)}>
              Интересно
            </button>
          </div>
        )}
        {card.kind === "challenge" && !doneCard && <Challenge seconds={Number(card.data.seconds ?? 0)} reps={Number(card.data.reps ?? 0)} onDone={() => finishCard(20)} />}
        {(card.kind === "fact" || card.kind === "challenge") && doneCard && <div className="rs-explain is-right"><b>{card.kind === "challenge" ? "Засчитано" : "Записал"}</b><span>{card.kind === "challenge" ? "Тело сказало спасибо — и тяга стала слабее." : "Завтра напомню — так запоминается надолго."}</span></div>}
      </article>

      <div className="rs-feed-foot">
        <span className="rs-hint">{settled ? "Enter, пробел или колесо — дальше" : isQuestion ? "Клавиши 1–4 — ответ" : ""}</span>
        <button type="button" className="btn btn-primary" disabled={!settled} onClick={next}>
          {i + 1 >= cards.length ? "Завершить" : "Дальше"}
        </button>
      </div>
    </div>
  );
}

function Challenge({ seconds, reps, onDone }: { seconds: number; reps: number; onDone: () => void }) {
  const [left, setLeft] = useState<number | null>(null);
  useEffect(() => {
    if (left === null) return;
    if (left <= 0) {
      onDone();
      return;
    }
    const id = window.setTimeout(() => setLeft(left - 1), 1000);
    return () => window.clearTimeout(id);
  }, [left, onDone]);
  return (
    <div className="rs-card-actions">
      {seconds > 0 ? (
        left === null ? (
          <button type="button" className="btn btn-primary" onClick={() => setLeft(seconds)}>
            Старт · {seconds} с
          </button>
        ) : (
          <span className="rs-countdown">{left} с</span>
        )
      ) : (
        <button type="button" className="btn btn-primary" onClick={onDone}>
          Сделал{reps ? ` ${reps}` : ""} ✓
        </button>
      )}
      <button type="button" className="btn btn-ghost" onClick={onDone}>
        Не сейчас
      </button>
    </div>
  );
}
