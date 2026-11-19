// «Стрик»: the user's real GitHub contribution calendar — their avatar, name
// and the same cells their profile shows, in JARVIS's warm amber instead of
// GitHub green. Data: `hub_github_get` (GraphQL through the signed-in GitHub
// CLI, or the public profile). Nothing here is made up.

import { useEffect, useMemo, useState, type CSSProperties, type FormEvent } from "react";
import { useReducedMotion } from "motion/react";
import { countUpValue, waveDelay } from "../motion";
import { localDateIso } from "../../store/hub";
import { useGithub } from "../../store/github";
import type { GithubActivity, GithubDay } from "../../lib/types";

const WEEKS = 52;
const MONTHS = ["янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
const MONTHS_GEN = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];

function plural(n: number, one: string, few: string, many: string): string {
  const d = n % 10;
  const h = n % 100;
  if (d === 1 && h !== 11) return one;
  if (d >= 2 && d <= 4 && (h < 12 || h > 14)) return few;
  return many;
}

const fmt = (n: number) => n.toLocaleString("ru-RU");

function dayDetail(d: GithubDay): string {
  const [, m, day] = d.date.split("-").map(Number);
  const when = `${day} ${MONTHS_GEN[(m ?? 1) - 1] ?? ""}`;
  if (d.count === 0) return `${when}: без вкладов`;
  return `${when}: ${fmt(d.count)} ${plural(d.count, "вклад", "вклада", "вкладов")}`;
}

/** Counts from 0 up to `target` once (rAF only while it runs). */
export function useCountUp(target: number, ms = 700): number {
  const reduced = useReducedMotion();
  const [v, setV] = useState(() => (reduced ? target : 0));
  useEffect(() => {
    if (reduced || target === 0) {
      setV(target);
      return;
    }
    const t0 = performance.now();
    let raf = requestAnimationFrame(function step(now) {
      const t = Math.min(1, (now - t0) / ms);
      setV(countUpValue(target, t));
      if (t < 1) raf = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(raf);
  }, [target, ms, reduced]);
  return v;
}

interface Cell {
  iso: string;
  future: boolean;
  today: boolean;
  day: GithubDay;
}

/** 52 Monday-first weeks ending with the current one, plus month labels. */
function useGrid(activity: GithubActivity | null | undefined) {
  return useMemo(() => {
    const byDate = new Map((activity?.days ?? []).map((d) => [d.date, d]));
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const todayIso = localDateIso(today);
    const back = (today.getDay() + 6) % 7;
    const start = new Date(today.getFullYear(), today.getMonth(), today.getDate() - back - (WEEKS - 1) * 7);
    const weeks: Cell[][] = [];
    const months: Array<{ col: number; label: string }> = [];
    let lastMonth = -1;
    for (let w = 0; w < WEEKS; w++) {
      const col: Cell[] = [];
      for (let r = 0; r < 7; r++) {
        const dt = new Date(start.getFullYear(), start.getMonth(), start.getDate() + w * 7 + r);
        const iso = localDateIso(dt);
        if (r === 0 && dt.getMonth() !== lastMonth) {
          lastMonth = dt.getMonth();
          // A month that starts in the last two columns has no room for its name.
          if (w > 0 || dt.getDate() <= 7) months.push({ col: w, label: MONTHS[lastMonth] ?? "" });
        }
        col.push({ iso, future: dt > today, today: iso === todayIso, day: byDate.get(iso) ?? { date: iso, count: 0, level: 0 } });
      }
      weeks.push(col);
    }
    const spaced = months.filter((m, i) => i === 0 || m.col - months[i - 1]!.col >= 3);
    return { weeks, months: spaced };
  }, [activity]);
}

export function ActivityDeck() {
  const st = useGithub((s) => s.state);
  const loading = useGithub((s) => s.loading);
  const load = useGithub((s) => s.load);
  const [hovered, setHovered] = useState<GithubDay | null>(null);

  useEffect(() => {
    void load(false);
  }, [load]);

  const activity = st?.activity ?? null;
  const { weeks, months } = useGrid(activity);
  const total = useCountUp(activity?.total ?? 0);
  const streak = useCountUp(activity?.currentStreak ?? 0);
  const longest = useCountUp(activity?.longestStreak ?? 0);
  const hasAny = (activity?.total ?? 0) > 0;
  const lastPast = weeks.length - 1;

  if (!activity) {
    return st === null && loading ? (
      <div className="streak-deck streak-empty">
        <span className="streak-muted">Загружаю календарь GitHub…</span>
      </div>
    ) : (
      <ConnectGithub />
    );
  }

  return (
    <div className={`streak-deck${hasAny ? " is-wave" : ""}`} style={{ "--flash-delay": `${waveDelay(lastPast, weeks.length) + 0.2}s` } as CSSProperties}>
      <header className="streak-head">
        <span className="streak-who" title={activity.profileUrl}>
          {activity.avatar ? (
            <img className="streak-avatar" src={activity.avatar} alt="" />
          ) : (
            <span className="streak-avatar is-letter">{activity.name.charAt(0).toUpperCase()}</span>
          )}
          <span className="streak-name">{activity.name}</span>
          {activity.name.toLowerCase() !== activity.login.toLowerCase() && <span className="streak-login">@{activity.login}</span>}
        </span>
        <span
          className="streak-total"
          title={activity.source === "gh" ? "Включая приватные репозитории" : "Только публичные вклады: войдите в GitHub CLI (gh auth login), чтобы учитывать приватные"}
        >
          <strong>{fmt(total)}</strong> {plural(activity.total, "вклад", "вклада", "вкладов")} за год
        </span>
      </header>

      <div className="streak-matrix" onMouseLeave={() => setHovered(null)}>
        <div className="streak-months" aria-hidden="true">
          {months.map((m) => (
            <span key={`${m.col}-${m.label}`} style={{ left: `${m.col * 10}px` }}>
              {m.label}
            </span>
          ))}
        </div>
        <div className="streak-grid" role="img" aria-label={`Календарь вкладов GitHub: ${fmt(activity.total)} за год`}>
          {weeks.map((col, w) => (
            <div key={w} className="streak-col" style={{ "--wave-delay": `${waveDelay(w, weeks.length)}s` } as CSSProperties}>
              {col.map((c) => (
                <span
                  key={c.iso}
                  className={`streak-cell ${c.future ? "is-future" : `l${Math.min(4, Math.max(0, c.day.level))}`}${c.today ? " is-today" : ""}`}
                  onMouseEnter={() => !c.future && setHovered(c.day)}
                />
              ))}
            </div>
          ))}
        </div>
      </div>

      <footer className="streak-foot">
        {hovered ? (
          <span className="streak-detail">{dayDetail(hovered)}</span>
        ) : (
          <span className="streak-stats">
            <span>
              Серия <strong>{streak}</strong> {plural(activity.currentStreak, "день", "дня", "дней")}
            </span>
            <span>
              Рекорд <strong>{longest}</strong> {plural(activity.longestStreak, "день", "дня", "дней")}
            </span>
          </span>
        )}
        <span className="streak-legend" aria-hidden="true">
          {st?.error ? (
            <button type="button" className="streak-retry" title={st.error} onClick={() => void load(true)}>
              Повторить
            </button>
          ) : (
            <>
              меньше
              <i className="streak-cell l0" />
              <i className="streak-cell l1" />
              <i className="streak-cell l2" />
              <i className="streak-cell l3" />
              <i className="streak-cell l4" />
              больше
            </>
          )}
        </span>
      </footer>
    </div>
  );
}

function ConnectGithub() {
  const st = useGithub((s) => s.state);
  const loading = useGithub((s) => s.loading);
  const formError = useGithub((s) => s.formError);
  const connect = useGithub((s) => s.connect);
  const [login, setLogin] = useState(st?.login ?? "");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (login.trim()) void connect(login);
  };

  return (
    <form className="streak-deck streak-empty" onSubmit={submit}>
      <p className="streak-pitch">Ваш календарь вкладов с GitHub: серия дней, рекорд, коммиты за год.</p>
      <div className="streak-connect">
        <span className="streak-at">github.com/</span>
        <input
          value={login}
          onChange={(e) => setLogin(e.target.value)}
          placeholder="логин"
          spellCheck={false}
          autoComplete="off"
          aria-label="Логин GitHub"
        />
        <button type="submit" className="streak-connect-btn" disabled={loading || !login.trim()}>
          {loading ? "Подключаю…" : "Подключить"}
        </button>
      </div>
      <span className={`streak-muted${formError || st?.error ? " is-error" : ""}`}>
        {formError || st?.error || "Если вы вошли в GitHub CLI, приватные вклады тоже посчитаются."}
      </span>
    </form>
  );
}
