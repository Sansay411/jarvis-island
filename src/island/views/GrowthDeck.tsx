// «Рост»: the mentor at a glance. Left — level, streak and today's XP
// towards a counted day; right — short-video minutes left this day and the
// week as seven bars. Buttons: «Перезагрузка», the planner, the year map.

import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../../lib/ipc";
import { useIsland } from "../../store/island";
import { daysRu, mentorIpc, minutesRu, useMentor } from "../../store/mentor";

const WD = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];

export function GrowthDeck() {
  const st = useMentor((s) => s.state);
  const setDeck = useIsland((s) => s.setDeck);
  if (!st) return <div className="growth-deck is-empty">Наставник просыпается…</div>;
  const levelPct = Math.min(100, ((st.xpTotal - st.levelFrom) / Math.max(1, st.levelTo - st.levelFrom)) * 100);
  const dayPct = Math.min(100, (st.todayXp / Math.max(1, st.goalXp)) * 100);
  const left = Math.max(0, st.short.limitSec - st.short.usedSec);
  const maxXp = Math.max(st.goalXp, ...st.week.map((d) => d.xp), 1);
  return (
    <div className="growth-deck">
      <div className="growth-col">
        <div className="deck-label">Уровень {st.level} · {st.levelName}</div>
        <div className="growth-streak">
          <b>{st.streak}</b>
          <span>
            {daysRu(st.streak).replace(/^\d+ /, "")} подряд
            <small>рекорд {st.bestStreak}</small>
          </span>
        </div>
        <div className="growth-bar" aria-label="Прогресс уровня">
          <i style={{ width: `${levelPct}%` }} />
        </div>
        <div className="growth-line">
          <span>Сегодня {st.todayXp} XP</span>
          <span>{st.todayXp >= st.goalXp ? "день засчитан" : `до зачёта ${st.goalXp - st.todayXp}`}</span>
        </div>
        <div className="growth-bar is-day">
          <i style={{ width: `${dayPct}%` }} />
        </div>
      </div>
      <div className="nook-hairline" aria-hidden="true" />
      <div className="growth-col">
        <div className="deck-label">Короткие видео</div>
        <div className="growth-video">
          {st.strict ? (
            <>
              <b>закрыты</b>
              <span>{st.strict}</span>
            </>
          ) : st.guard ? (
            <>
              <b>{minutesRu(Math.ceil(left / 60))}</b>
              <span>осталось на сегодня{st.earnedSec > 0 ? ` · заработано +${Math.round(st.earnedSec / 60)}` : ""}</span>
            </>
          ) : (
            <>
              <b>без контроля</b>
              <span>включается в настройках, раздел «Наставник»</span>
            </>
          )}
        </div>
        <div className="growth-week">
          {st.week.map((d) => (
            <span key={d.date} className={`growth-day${d.growth ? " is-growth" : ""}`} title={`${d.xp} XP`}>
              <i style={{ height: `${Math.max(8, (d.xp / maxXp) * 100)}%` }} />
              <em>{WD[(new Date(d.date + "T12:00").getDay() + 6) % 7]}</em>
            </span>
          ))}
        </div>
        <div className="growth-actions">
          <button type="button" className="deck-btn is-accent" onClick={() => void mentorIpc.openReset()}>
            Перезагрузка
          </button>
          <button type="button" className="deck-btn" onClick={() => isTauri() && void invoke("planner_open", { date: null })}>
            Календарь
          </button>
          <button type="button" className="deck-btn" onClick={() => setDeck("activity")}>
            За год
          </button>
        </div>
      </div>
    </div>
  );
}
