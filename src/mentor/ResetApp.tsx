// «Перезагрузка» — the window JARVIS opens when it closes a short-video
// tab (or when asked). Same look as the installer. It names what happened
// without shaming, shows the user's own if-then plan, asks what pulls
// (bored, tired, avoiding a task, resting) and offers four quick rewards
// that leave you stronger: the card feed, a game, a workout, ten minutes on
// a project. Then the reward screen bridges into real work.

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "../lib/ipc";
import heroUrl from "../../installer/ui/hero.png";
import { daysRu, mentorIpc, minutesRu, startMentorFeed, useMentor, type Intervention, type Reward, type Step } from "../store/mentor";
import { Feed, type FeedResult } from "./Feed";
import { Game, GamePicker, GAME_TITLE, GAME_TRACK, PUZZLES, type GameId } from "./Games";
import { Memory, Schulte, Sudoku } from "./Puzzles";
import { Breath, Workout } from "./Body";
import { ART } from "./art";
import { Icon, type IconName } from "../ui/Icon";
import "./reset.css";

type Mood = "bored" | "tired" | "avoid" | "rest" | null;
type Screen =
  | { id: "intro" }
  | { id: "feed" }
  | { id: "games" }
  | { id: "game"; game: GameId }
  | { id: "workout" }
  | { id: "breath" }
  | { id: "step" }
  | { id: "started"; title: string; minutes: number }
  | { id: "done"; reward: Reward; extraXp: number; kind: string; note: string };

const MOODS: Array<{ id: Exclude<Mood, null>; label: string; icon: IconName }> = [
  { id: "bored", label: "Скучно", icon: "meh" },
  { id: "tired", label: "Устал", icon: "battery" },
  { id: "avoid", label: "Не хочу браться за задачу", icon: "mountain" },
  { id: "rest", label: "Просто отдыхаю", icon: "cup" },
];

const STEP_KIND: Record<string, { label: string; icon: IconName }> = {
  repo: { label: "Код", icon: "code" },
  event: { label: "Календарь", icon: "calendar" },
  task: { label: "Задача", icon: "check" },
};

const RECOMMEND: Record<Exclude<Mood, null>, { tile: string; line: string }> = {
  bored: { tile: "feed", line: "Скуке нужна новизна. Лента даст её за три минуты — и что-то останется в голове." },
  tired: { tile: "workout", line: "Усталость после экрана снимает движение, а не ещё один экран. Три упражнения — и бодрее." },
  avoid: { tile: "step", line: "Тянет туда, где нет неприятного чувства. Сделаем первый шаг крошечным: десять минут, потом можно остановиться." },
  rest: { tile: "breath", line: "Отдых — это хорошо. Только лента не отдыхает мозг, а грузит его. Минута дыхания отдыхает лучше." },
};

function close() {
  if (isTauri()) void getCurrentWindow().close();
}

export default function ResetApp() {
  const [iv, setIv] = useState<Intervention | null>(null);
  const [screen, setScreen] = useState<Screen>({ id: "intro" });
  const [mood, setMood] = useState<Mood>(null);
  const state = useMentor((s) => s.state);

  useEffect(() => {
    void startMentorFeed();
    void mentorIpc.pending().then((p) => {
      if (!p) return;
      setIv(p);
      if (p.screen) setScreen({ id: p.screen } as Screen);
    });
    if (!isTauri()) {
      const q = new URLSearchParams(location.search);
      const reason = (q.get("reason") as Intervention["reason"]) ?? "budget";
      setIv({
        id: "qa",
        reason,
        site: q.get("site") ?? "tiktok",
        label: q.get("label") ?? "TikTok",
        usedSec: 612,
        strict: reason === "strict" ? "Идёт фокус «Rust: владение»" : null,
        why: "Тяга — волна: пик через пару минут, потом спад. Займи руки и голову на три минуты, и она сойдёт сама.",
        title: "Смешные коты | TikTok",
        url: null,
        closed: true,
        ifThen: q.get("plan") === "1" ? "сделаю 20 приседаний и одну карточку английского" : "",
        at: new Date().toISOString(),
      });
      const s = q.get("screen") as Screen["id"] | null;
      if (s && s !== "done" && s !== "started" && s !== "game") setScreen({ id: s } as Screen);
      if (s === "game") {
        const g = q.get("game") as GameId | null;
        setScreen({ id: "game", game: g && (Object.keys(GAME_TITLE) as GameId[]).includes(g) ? g : "sprint" });
      }
      const won = q.get("reward");
      if (s === "done") setScreen({ id: "done", reward: { xp: 55, earnedSec: 180, level: won === "level" ? 4 : 3, levelName: won === "level" ? "Собранный" : "Практик", levelUp: won === "level", streak: 6, best: won === "record", record: 0 }, extraXp: 48, kind: "feed", note: "5 из 6 верно" });
      if (s === "started") setScreen({ id: "started", title: "jarvis-island: открыть файл и написать заголовок функции", minutes: 10 });
      return;
    }
    const un = listen<Intervention>("mentor:reset", (e) => {
      setIv(e.payload);
      setScreen(e.payload.screen ? ({ id: e.payload.screen } as Screen) : { id: "intro" });
      setMood(null);
    });
    return () => void un.then((f) => f());
  }, []);

  const afterClose = Boolean(iv && iv.reason !== "voluntary");
  const finish = useCallback(
    async (kind: string, track: string, title: string, minutes: number, score: number, xp: number, extraXp: number, note: string) => {
      const reward = await mentorIpc.complete(kind, track, title, minutes, score, xp, afterClose);
      setScreen({ id: "done", reward, extraXp, kind, note });
    },
    [afterClose],
  );

  const voluntary = !iv || iv.reason === "voluntary";
  const usedMin = iv ? Math.max(1, Math.round(iv.usedSec / 60)) : 0;
  const headline = voluntary
    ? "Чем зарядиться?"
    : iv!.reason === "strict"
      ? `${iv!.strict ?? "Время фокуса"}. ${iv!.label} подождёт.`
      : `${iv!.label} — ${minutesRu(usedMin)}. ${iv!.closed ? "Я закрыл его." : "Пора остановиться."}`;
  const sub = voluntary ? "Три минуты, после которых голова свежая. Выбирай, что ближе." : iv!.why;
  const recommended = mood ? RECOMMEND[mood] : null;
  const learnable = Boolean(iv && iv.url && (iv.site === "youtube" || iv.site === "shorts"));

  const tiles = [
    { id: "feed", title: "Лента знаний", meta: "7 карточек · 3 мин", text: "Код, английский, тело. Как лента — только после неё ты умнее.", earn: "+3 мин видео" },
    { id: "games", title: "Игры для головы", meta: "Судоку, счёт, слова, Шульте", text: "Побей свой рекорд. Быстро, азартно и с пользой.", earn: "+2 мин видео" },
    { id: "workout", title: "Разминка", meta: "3 упражнения · 3 мин", text: "Движение снимает тягу лучше силы воли.", earn: "+3 мин видео" },
    { id: "step", title: "Шаг по проекту", meta: "10 минут фокуса", text: "Самое маленькое действие. Дальше пойдёт само.", earn: "+2 XP за минуту" },
  ];

  return (
    <div className="reset">
      <header className="bar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <span className="brand-mark" aria-hidden="true">
            <i />
            <i />
          </span>
          <span className="brand-name" data-tauri-drag-region>
            jarvis
          </span>
          <span className="brand-sub" data-tauri-drag-region>
            перезагрузка
          </span>
        </div>
        <div className="win">
          <button type="button" className="win-btn" aria-label="Свернуть" onClick={() => isTauri() && void getCurrentWindow().minimize()}>
            <svg viewBox="0 0 12 12">
              <path d="M2 6.5h8" />
            </svg>
          </button>
          <button type="button" className="win-btn is-close" aria-label="Закрыть" onClick={close}>
            <svg viewBox="0 0 12 12">
              <path d="m2.5 2.5 7 7M9.5 2.5l-7 7" />
            </svg>
          </button>
        </div>
      </header>

      <main className="rs-main">
        {screen.id === "intro" && (
          <section className="rs-screen rs-intro">
            <div className="rs-hero">
              <img src={heroUrl} alt="" draggable={false} />
            </div>
            <h1>{headline}</h1>
            {sub && <p className="lead">{recommended ? recommended.line : sub}</p>}
            {iv?.ifThen && (
              <p className="rs-plan">
                Утром ты решил: <b>«{iv.ifThen}»</b>
              </p>
            )}
            <div className="rs-moods" role="group" aria-label="Что тянет">
              <span>Что тянет?</span>
              {MOODS.map((m) => (
                <button key={m.id} type="button" className={`rs-mood${mood === m.id ? " is-on" : ""}`} onClick={() => setMood(mood === m.id ? null : m.id)}>
                  <Icon name={m.icon} size={15} />
                  {m.label}
                </button>
              ))}
            </div>
            <div className="rs-tiles">
              {tiles.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  className={`rs-tile${recommended?.tile === t.id ? " is-recommended" : ""}`}
                  onClick={() => setScreen({ id: t.id } as Screen)}
                >
                  {recommended?.tile === t.id && <span className="rs-tile-badge">Сейчас лучше всего</span>}
                  <img className="rs-tile-art" src={ART[t.id as keyof typeof ART]} alt="" draggable={false} />
                  <b>{t.title}</b>
                  <span className="rs-tile-meta">{t.meta}</span>
                  <span className="rs-tile-text">{t.text}</span>
                  <span className="rs-tile-earn">{t.earn}</span>
                </button>
              ))}
            </div>
            <div className="rs-links">
              <button type="button" className={`rs-pill${recommended?.tile === "breath" ? " is-strong" : ""}`} onClick={() => setScreen({ id: "breath" })}>
                <img src={ART.breath} alt="" draggable={false} />
                Минута дыхания
                <span>+1 мин видео</span>
              </button>
              {learnable && (
                <button
                  type="button"
                  className="rs-pill is-quiet"
                  onClick={() => {
                    void mentorIpc.allow(iv!.title, iv!.url).then(close);
                  }}
                >
                  <Icon name="book" size={16} />
                  Это была учёба — вернуть видео
                </button>
              )}
            </div>
          </section>
        )}

        {screen.id === "feed" && (
          <section className="rs-screen">
            <Feed
              onDone={(r: FeedResult) =>
                void finish("feed", "mix", "Лента знаний", r.minutes, r.right, 20, r.xp, r.answered ? `${r.right} из ${r.answered} верно${r.bestCombo >= 3 ? ` · серия ${r.bestCombo}` : ""}` : "")
              }
            />
          </section>
        )}
        {screen.id === "games" && (
          <section className="rs-screen rs-center">
            <h2 className="rs-h2">Игры для головы</h2>
            <GamePicker onPick={(game) => setScreen({ id: "game", game })} />
          </section>
        )}
        {screen.id === "game" && (
          <section className="rs-screen rs-center">
            {PUZZLES.includes(screen.game) ? (
              (() => {
                const g = screen.game;
                const done = (score: number, note: string) => void finish("game", GAME_TRACK[g], GAME_TITLE[g], 3, score, Math.min(90, Math.round(score / 3)), 0, note);
                return g === "sudoku" ? <Sudoku onDone={done} /> : g === "schulte" ? <Schulte onDone={done} /> : <Memory onDone={done} />;
              })()
            ) : (
              <Game game={screen.game} onDone={(score) => void finish("game", GAME_TRACK[screen.game], GAME_TITLE[screen.game], 1, score, Math.min(60, score * 2), 0, `${score} очков`)} />
            )}
          </section>
        )}
        {screen.id === "workout" && (
          <section className="rs-screen">
            <Workout onDone={(n) => void finish("workout", "body", "Разминка", 3, n, n * 15, 0, `${n} из 3 упражнений`)} />
          </section>
        )}
        {screen.id === "breath" && (
          <section className="rs-screen rs-center">
            <Breath onDone={() => void finish("breath", "body", "Дыхание", 1, 6, 15, 0, "6 циклов")} />
          </section>
        )}
        {screen.id === "step" && <StepScreen onStarted={(title, minutes) => setScreen({ id: "started", title, minutes })} />}
        {screen.id === "started" && (
          <section className="rs-screen rs-center rs-done">
            <div className="rs-hero is-small is-art">
              <img src={ART.step} alt="" draggable={false} />
            </div>
            <h1>Фокус пошёл</h1>
            <p className="lead">
              «{screen.title}» — {minutesRu(screen.minutes)}. Таймер на острове, короткие видео закрыты до конца. Начать — самое трудное, дальше обычно тянет продолжить.
            </p>
          </section>
        )}
        {screen.id === "done" && (
          <section className="rs-screen rs-center rs-done">
            <div className={`rs-hero is-small${screen.reward.levelUp || screen.reward.best ? " is-art" : ""}`}>
              <img src={screen.reward.levelUp ? ART.level : screen.reward.best ? ART.trophy : heroUrl} alt="" draggable={false} />
            </div>
            <h1>{screen.reward.levelUp ? `Новый уровень: ${screen.reward.levelName}` : screen.reward.best ? "Новый рекорд!" : "Готово"}</h1>
            {screen.note && <p className="lead lead-tight">{screen.note}</p>}
            <div className="rs-rewards">
              <span className="rs-reward is-xp">
                <Icon name="spark" size={17} />+{screen.reward.xp + screen.extraXp} XP
              </span>
              {screen.reward.earnedSec > 0 && (
                <span className="rs-reward">
                  <Icon name="reels" size={17} />+{minutesRu(Math.round(screen.reward.earnedSec / 60))} видео
                </span>
              )}
              {screen.reward.streak > 0 && (
                <span className="rs-reward">
                  <img className="rs-reward-art" src={ART.streak} alt="" draggable={false} />
                  Серия {daysRu(screen.reward.streak)}
                </span>
              )}
            </div>
            {state && (
              <div className="rs-level">
                <div className="rs-level-row">
                  <b>
                    Уровень {state.level} · {state.levelName}
                  </b>
                  <span>
                    {state.xpTotal} / {state.levelTo} XP
                  </span>
                </div>
                <div className="rs-level-bar">
                  <i style={{ width: `${Math.min(100, ((state.xpTotal - state.levelFrom) / Math.max(1, state.levelTo - state.levelFrom)) * 100)}%` }} />
                </div>
              </div>
            )}
            <p className="rs-bridge">{screen.kind === "step" ? "" : "Голова разогрета — самое время для дела. Десять минут на проекте?"}</p>
          </section>
        )}
      </main>

      <footer className="foot">
        <span className="version">
          {state && screen.id !== "done" && (
            <>
              <span>
                <Icon name="rank" size={15} />
                Уровень {state.level} · {state.levelName}
              </span>
              <span className={state.todayXp >= state.goalXp ? "is-lit" : undefined}>
                <Icon name="flame" size={15} />
                Серия {daysRu(state.streak)}
              </span>
              <span>
                <Icon name="spark" size={15} />
                {state.todayXp < state.goalXp ? `До зачёта дня ${state.goalXp - state.todayXp} XP` : "День засчитан"}
              </span>
            </>
          )}
        </span>
        <div className="foot-actions">
          {screen.id !== "intro" && screen.id !== "done" && screen.id !== "started" && (
            <button type="button" className="btn btn-ghost" onClick={() => setScreen({ id: "intro" })}>
              Назад
            </button>
          )}
          {screen.id === "done" && (
            <>
              <button type="button" className="btn btn-ghost" onClick={() => setScreen({ id: "feed" })}>
                Ещё карточки
              </button>
              <button type="button" className="btn btn-primary" onClick={() => setScreen({ id: "step" })}>
                Шаг по проекту
              </button>
            </>
          )}
          <button type="button" className={`btn ${screen.id === "intro" || screen.id === "started" ? "btn-primary" : "btn-ghost"}`} onClick={close}>
            {screen.id === "started" ? "Поехали" : "Закрыть"}
          </button>
        </div>
      </footer>
    </div>
  );
}

function StepScreen({ onStarted }: { onStarted: (title: string, minutes: number) => void }) {
  const [steps, setSteps] = useState<Step[] | null>(null);
  const [pick, setPick] = useState(0);
  const [own, setOwn] = useState("");
  const [tiny, setTiny] = useState("");
  const [minutes, setMinutes] = useState(10);
  useEffect(() => {
    void mentorIpc.steps().then(setSteps);
  }, []);
  const chosen = own.trim() ? null : steps?.[pick] ?? null;
  const title = own.trim() || chosen?.title || "";
  const start = async () => {
    if (!title) return;
    const full = tiny.trim() ? `${title}: ${tiny.trim()}` : title;
    await mentorIpc.startStep(full, minutes, chosen?.path ?? null);
    onStarted(full, minutes);
  };
  return (
    <section className="rs-screen rs-step">
      <h2 className="rs-h2">Шаг по проекту</h2>
      <p className="lead lead-left">Выбери, над чем десять минут. Я запущу фокус и открою проект.</p>
      <div className="rs-step-list" role="radiogroup">
        {steps === null && <div className="rs-loading">Смотрю календарь, задачи и репозитории…</div>}
        {steps?.map((s, k) => (
          <button key={k} type="button" role="radio" aria-checked={!own.trim() && pick === k} className={`rs-step-item${!own.trim() && pick === k ? " is-on" : ""}`} onClick={() => { setOwn(""); setPick(k); }}>
            <span className={`rs-step-kind is-${s.kind}`}>
              <Icon name={(STEP_KIND[s.kind] ?? STEP_KIND.task!).icon} size={15} />
              {(STEP_KIND[s.kind] ?? STEP_KIND.task!).label}
            </span>
            <span className="rs-step-text">
              <b>{s.title}</b>
              <small>{s.subtitle}</small>
            </span>
          </button>
        ))}
        <input className="rs-input" placeholder="Или своё: над чем поработать" value={own} onChange={(e) => setOwn(e.target.value)} />
      </div>
      <div className="rs-step-row">
        <input className="rs-input" placeholder="Первый крошечный шаг: например, «открыть файл и написать заголовок функции»" value={tiny} onChange={(e) => setTiny(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void start()} />
        <div className="rs-chips">
          {[10, 15, 25].map((m) => (
            <button key={m} type="button" className={`rs-mood${minutes === m ? " is-on" : ""}`} onClick={() => setMinutes(m)}>
              {m} мин
            </button>
          ))}
        </div>
        <button type="button" className="btn btn-primary" disabled={!title} onClick={() => void start()}>
          Начать фокус
        </button>
      </div>
    </section>
  );
}
