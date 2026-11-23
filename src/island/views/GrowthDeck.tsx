// «Рост»: the mentor at a glance and at hand. Left — today's XP on a ring
// towards a counted day, the streak and the level; right — the short-video
// minutes left and the week. Below — what to do right now: a card answered
// right here, a workout, a breath, a project step or all of «Перезагрузка».

import { useEffect, useRef, useState } from "react";
import { Icon, type IconName } from "../../ui/Icon";
import { useIsland } from "../../store/island";
import { emote } from "../../store/emote";
import { daysRu, mentorIpc, minutesRu, useMentor, type FeedCard, type ResetScreen } from "../../store/mentor";

const WD = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];

const QUICK: Array<{ id: "card" | ResetScreen; label: string; icon: IconName }> = [
  { id: "card", label: "Карточка", icon: "cards" },
  { id: "workout", label: "Разминка", icon: "dumbbell" },
  { id: "breath", label: "Дыхание", icon: "wind" },
  { id: "step", label: "Шаг по проекту", icon: "rocket" },
];

const TRACK: Record<string, { label: string; icon: IconName }> = {
  code: { label: "Код", icon: "code" },
  english: { label: "English", icon: "book" },
  body: { label: "Тело", icon: "heart" },
};

function DayRing({ xp, goal }: { xp: number; goal: number }) {
  const size = 84;
  const stroke = 6;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const done = xp >= goal;
  return (
    <div className={`growth-ring${done ? " is-done" : ""}`} title={done ? `Сегодня ${xp} XP — день засчитан` : `Сегодня ${xp} из ${goal} XP`}>
      <svg className="growth-ring-svg" viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <circle cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} className="growth-ring-track" />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          strokeWidth={stroke}
          className="growth-ring-fill"
          strokeDasharray={c}
          strokeDashoffset={c * (1 - Math.min(1, xp / Math.max(1, goal)))}
        />
      </svg>
      <span className="growth-ring-num">
        <b>{xp}</b>
        <small>{done ? "засчитан" : `из ${goal} XP`}</small>
      </span>
    </div>
  );
}

/** A card that fits the island: a question with short answers or a word. */
function fits(c: FeedCard): boolean {
  if (c.kind === "word") return true;
  if (c.kind !== "quiz") return false;
  const q = String(c.data.q ?? "");
  const opts = (c.data.options as string[] | undefined) ?? [];
  return q.length <= 130 && opts.length === 4 && opts.every((o) => o.length <= 40);
}

function QuickCard({ onClose }: { onClose: () => void }) {
  const [cards, setCards] = useState<FeedCard[] | null>(null);
  const [i, setI] = useState(0);
  const [picked, setPicked] = useState<number | null>(null);
  const [earned, setEarned] = useState(0);
  const shownAt = useRef(performance.now());

  const load = async () => {
    setCards(null);
    const set = (await mentorIpc.feed(8).catch(() => [] as FeedCard[])).filter(fits);
    setCards(set);
    setI(0);
    setPicked(null);
    shownAt.current = performance.now();
  };
  useEffect(() => {
    void load();
  }, []);

  if (cards === null) return <div className="growth-card is-loading">Подбираю карточку…</div>;
  const card = cards[i];
  if (!card) {
    return (
      <div className="growth-card is-empty">
        <span>Короткие карточки на сегодня кончились — в ленте их больше.</span>
        <div className="growth-card-foot">
          <button type="button" className="deck-btn" onClick={onClose}>
            Назад
          </button>
          <button type="button" className="deck-btn is-accent" onClick={() => void mentorIpc.openReset("feed")}>
            <Icon name="cards" size={12} />
            Открыть ленту
          </button>
        </div>
      </div>
    );
  }

  const word = card.kind === "word";
  const options = (card.data.options as string[] | undefined) ?? [];
  const answer = typeof card.data.answer === "number" ? card.data.answer : -1;
  const track = TRACK[card.track] ?? TRACK.code!;
  const right = picked !== null && picked === answer;
  const explain = word ? [card.data.ex, card.data.exRu].filter(Boolean).join(" — ") : String(card.data.explain ?? "");

  const choose = (k: number) => {
    if (picked !== null) return;
    setPicked(k);
    const ok = k === answer;
    const xp = ok ? 10 * (card.rare ? 3 : 1) : 3;
    setEarned((e) => e + xp);
    if (ok) emote(card.rare ? "star" : "wink");
    void mentorIpc.answer(card.id, ok, performance.now() - shownAt.current < 6000, xp);
  };
  const next = () => {
    if (i + 1 >= cards.length) {
      void load();
      return;
    }
    setI(i + 1);
    setPicked(null);
    shownAt.current = performance.now();
  };

  return (
    <div className={`growth-card${picked !== null ? (right ? " is-right" : " is-wrong") : ""}`}>
      <div className="growth-card-head">
        <span className="growth-card-track">
          <Icon name={track.icon} size={12} />
          {track.label}
          {card.review && <em>повторение</em>}
          {card.rare && <em className="is-rare">редкая ×3</em>}
        </span>
        {earned > 0 && <span className="growth-card-xp">+{earned} XP</span>}
        <button type="button" className="growth-card-close" aria-label="Закрыть карточку" onClick={onClose}>
          <Icon name="close" size={12} />
        </button>
      </div>
      {word ? (
        <div className="growth-card-q is-word">
          <b>{String(card.data.word ?? "")}</b>
          {card.data.ipa ? <span>/{String(card.data.ipa)}/</span> : null}
          <small>как переводится?</small>
        </div>
      ) : (
        <div className="growth-card-q">{String(card.data.q ?? "")}</div>
      )}
      <div className="growth-card-options" role="group" aria-label="Варианты ответа">
        {options.map((o, k) => (
          <button
            key={k}
            type="button"
            disabled={picked !== null}
            className={`growth-option${picked !== null && k === answer ? " is-answer" : ""}${picked === k && k !== answer ? " is-miss" : ""}`}
            onClick={() => choose(k)}
          >
            <span className="growth-option-key">{k + 1}</span>
            <span className="growth-option-text">{o}</span>
          </button>
        ))}
      </div>
      <div className="growth-card-foot">
        {picked === null ? (
          <span className="growth-card-hint">Верно — +{10 * (card.rare ? 3 : 1)} XP, ошибка вернётся завтра</span>
        ) : (
          <span className="growth-card-explain" title={explain}>
            <b>{right ? "Верно." : "Не то."}</b> {explain}
          </span>
        )}
        {picked !== null && (
          <button type="button" className="deck-btn is-accent" onClick={next}>
            Ещё
            <Icon name="chevron" size={11} />
          </button>
        )}
      </div>
    </div>
  );
}

export function GrowthDeck() {
  const st = useMentor((s) => s.state);
  const setDeck = useIsland((s) => s.setDeck);
  const cardOpen = useIsland((s) => s.growthCard);
  const setIsland = useIsland((s) => s.set);

  if (!st) return <div className="growth-deck is-empty">Наставник просыпается…</div>;
  if (cardOpen) return <QuickCard onClose={() => setIsland({ growthCard: false })} />;

  const levelPct = Math.min(100, ((st.xpTotal - st.levelFrom) / Math.max(1, st.levelTo - st.levelFrom)) * 100);
  const left = Math.max(0, st.short.limitSec - st.short.usedSec);
  const usedPct = Math.min(100, (st.short.usedSec / Math.max(1, st.short.limitSec)) * 100);
  const maxXp = Math.max(st.goalXp, ...st.week.map((d) => d.xp), 1);
  const todayIso = st.week.at(-1)?.date;
  const counted = st.todayXp >= st.goalXp;

  const videoValue = !st.guard ? "без контроля" : st.strict ? "закрыты" : minutesRu(Math.ceil(left / 60));
  const videoSub = !st.guard
    ? "включается в настройках, раздел «Наставник»"
    : st.strict
      ? st.strict
      : `осталось на сегодня${st.earnedSec > 0 ? ` · заработано +${Math.round(st.earnedSec / 60)} мин` : ""}`;

  const quick = (id: (typeof QUICK)[number]["id"]) => {
    if (id === "card") setIsland({ growthCard: true });
    else void mentorIpc.openReset(id);
  };

  return (
    <div className="growth-deck">
      <div className="growth-top">
        <div className="growth-col growth-me">
          <DayRing xp={st.todayXp} goal={st.goalXp} />
          <div className="growth-facts">
            <div className={`growth-streak${counted ? " is-lit" : ""}`} title={counted ? "Сегодня засчитан" : `До зачёта дня ${st.goalXp - st.todayXp} XP`}>
              <Icon name="flame" size={17} />
              <b>{st.streak}</b>
              <span>
                {daysRu(st.streak).replace(/^\d+ /, "")} подряд
                <small>рекорд {st.bestStreak}</small>
              </span>
            </div>
            <div className="growth-level">
              <span className="growth-level-name">
                <Icon name="rank" size={13} />
                Уровень {st.level} · {st.levelName}
              </span>
              <div className="growth-bar" aria-label="Прогресс уровня">
                <i style={{ width: `${levelPct}%` }} />
              </div>
              <span className="growth-level-next">{Math.max(0, st.levelTo - st.xpTotal)} XP до следующего</span>
            </div>
          </div>
        </div>

        <div className="nook-hairline" aria-hidden="true" />

        <div className="growth-col growth-video">
          <div className="growth-video-head">
            <span className="deck-label">
              <Icon name="reels" size={11} />
              Короткие видео
            </span>
            <b className={st.guard && !st.strict && left < 60 ? "is-low" : undefined}>{videoValue}</b>
          </div>
          <div className={`growth-budget${!st.guard ? " is-off" : st.strict ? " is-closed" : left < 60 ? " is-low" : ""}`}>
            <i style={{ width: `${st.guard && !st.strict ? usedPct : 100}%` }} />
          </div>
          <span className="growth-sub">{videoSub}</span>
          <button type="button" className="growth-week" onClick={() => setDeck("activity")} title="Карта года">
            {st.week.map((d) => (
              <span
                key={d.date}
                className={`growth-day${d.growth ? " is-growth" : ""}${d.date === todayIso ? " is-today" : ""}`}
                title={`${d.xp} XP · фокус ${d.focusMin} мин · короткие ${d.shortMin} мин`}
              >
                <i style={{ height: `${Math.max(8, (d.xp / maxXp) * 100)}%` }} />
                <em>{WD[(new Date(d.date + "T12:00").getDay() + 6) % 7]}</em>
              </span>
            ))}
          </button>
        </div>
      </div>

      <div className="growth-actions">
        {QUICK.map((q) => (
          <button key={q.id} type="button" className="growth-quick" onClick={() => quick(q.id)}>
            <Icon name={q.icon} size={13} />
            {q.label}
          </button>
        ))}
        <span className="growth-actions-gap" />
        <button type="button" className="deck-btn is-accent" onClick={() => void mentorIpc.openReset()}>
          <Icon name="restart" size={12} />
          Перезагрузка
        </button>
      </div>
    </div>
  );
}
