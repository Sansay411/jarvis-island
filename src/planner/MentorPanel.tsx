// The planner's right panel: JARVIS as a mentor. Level and streak, today's
// XP towards a counted day, the short-video minutes left (and earned), the
// week at a glance, the user's own «если потянет — то я…» plan and the
// evening review: what went well, what I learned, tomorrow's main thing.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../lib/ipc";
import { daysRu, mentorIpc, minutesRu, startMentorFeed, useMentor } from "../store/mentor";
import { usePlanner } from "../store/planner";

const WD = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];

export function MentorPanel({ now }: { now: Date }) {
  const st = useMentor((s) => s.state);
  const showToast = usePlanner((s) => s.showToast);
  const [ifThen, setIfThen] = useState("");
  const [review, setReview] = useState(false);
  const [r, setR] = useState({ wentWell: "", learned: "", tomorrow: "" });
  useEffect(() => {
    void startMentorFeed();
  }, []);
  useEffect(() => {
    if (st) setIfThen(st.ifThen);
  }, [st?.ifThen]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!st) return <aside className="pl-mentor" />;
  const levelPct = Math.min(100, ((st.xpTotal - st.levelFrom) / Math.max(1, st.levelTo - st.levelFrom)) * 100);
  const dayPct = Math.min(100, (st.todayXp / Math.max(1, st.goalXp)) * 100);
  const shortLeft = Math.max(0, st.short.limitSec - st.short.usedSec);
  const maxXp = Math.max(st.goalXp, ...st.week.map((d) => d.xp));
  const evening = now.getHours() >= 18;

  const saveIfThen = async () => {
    if (!isTauri()) return;
    try {
      const view = await invoke<{ settings: Record<string, unknown> & { mentor: Record<string, unknown> } }>("settings_get");
      const settings = { ...view.settings, mentor: { ...view.settings.mentor, ifThen: ifThen.trim() } };
      await invoke("settings_save", { settings });
      showToast("План запомнил — напомню, когда потянет");
    } catch {
      showToast("Не удалось сохранить план");
    }
  };

  return (
    <aside className="pl-mentor">
      <div className="pl-mentor-head">
        <span className="pl-eyes" aria-hidden="true">
          <i />
          <i />
        </span>
        <b>Наставник</b>
      </div>

      <div className="pl-m-card">
        <div className="pl-m-row">
          <b>
            Уровень {st.level} · {st.levelName}
          </b>
          <span>
            {st.xpTotal} / {st.levelTo}
          </span>
        </div>
        <div className="pl-m-bar">
          <i style={{ width: `${levelPct}%` }} />
        </div>
        <div className="pl-m-stats">
          <div>
            <b>{daysRu(st.streak)}</b>
            <span>серия · рекорд {st.bestStreak}</span>
          </div>
          <div>
            <b>{st.todayXp} XP</b>
            <span>{st.todayXp >= st.goalXp ? "день засчитан" : `ещё ${st.goalXp - st.todayXp} до зачёта`}</span>
          </div>
        </div>
        <div className="pl-m-bar is-day">
          <i style={{ width: `${dayPct}%` }} />
        </div>
      </div>

      <div className="pl-m-card">
        <div className="pl-m-row">
          <b>Короткие видео</b>
          <span>{st.guard ? (st.strict ? "закрыты" : `осталось ${minutesRu(Math.ceil(shortLeft / 60))}`) : "без контроля"}</span>
        </div>
        <div className="pl-m-bar is-video">
          <i style={{ width: `${Math.min(100, (st.short.usedSec / Math.max(1, st.short.limitSec)) * 100)}%` }} />
        </div>
        <p className="pl-m-note">
          {st.strict
            ? `${st.strict} — TikTok и Shorts закрываются сразу.`
            : st.earnedSec > 0
              ? `Заработано сегодня +${minutesRu(Math.round(st.earnedSec / 60))}: польза открывает минуты.`
              : "Карточки, разминка и фокус добавляют минуты."}
          {st.closesToday > 0 ? ` Закрыто сегодня: ${st.closesToday}.` : ""}
        </p>
        <button type="button" className="pl-m-btn" onClick={() => void mentorIpc.openReset()}>
          Перезагрузка · 3 минуты
        </button>
      </div>

      {st.week.length > 0 && (
        <div className="pl-m-card">
          <div className="pl-m-row">
            <b>Неделя</b>
            <span>{st.week.filter((d) => d.growth).length} из 7 дней роста</span>
          </div>
          <div className="pl-m-week">
            {st.week.map((d) => {
              const wd = (new Date(d.date + "T12:00").getDay() + 6) % 7;
              return (
                <div key={d.date} className={`pl-m-day${d.growth ? " is-growth" : ""}`} title={`${d.xp} XP · фокус ${d.focusMin} мин · короткие ${d.shortMin} мин`}>
                  <span className="pl-m-col">
                    <i style={{ height: `${Math.max(4, (d.xp / maxXp) * 100)}%` }} />
                  </span>
                  <span>{WD[wd]}</span>
                </div>
              );
            })}
          </div>
        </div>
      )}

      <div className="pl-m-card">
        <div className="pl-m-row">
          <b>Если потянет в ленту, то я…</b>
        </div>
        <input
          className="pl-field"
          placeholder="сделаю 20 приседаний и одну карточку"
          value={ifThen}
          onChange={(e) => setIfThen(e.target.value)}
          onBlur={() => ifThen.trim() !== st.ifThen && void saveIfThen()}
          onKeyDown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
        />
        <p className="pl-m-note">Свой план «если — то» срабатывает в 2–3 раза чаще простого намерения. Джарвис покажет его в момент тяги.</p>
      </div>

      {(evening || review) && (
        <div className="pl-m-card">
          <div className="pl-m-row">
            <b>Итог дня</b>
            {!review && (
              <button type="button" className="pl-btn-text is-sm" onClick={() => setReview(true)}>
                Начать
              </button>
            )}
          </div>
          {review ? (
            <>
              <textarea className="pl-field pl-notes" rows={2} placeholder="Что получилось сегодня?" value={r.wentWell} onChange={(e) => setR({ ...r, wentWell: e.target.value })} />
              <textarea className="pl-field pl-notes" rows={2} placeholder="Что я узнал? (своими словами — так запоминается)" value={r.learned} onChange={(e) => setR({ ...r, learned: e.target.value })} />
              <input className="pl-field" placeholder="Главное на завтра — одно дело" value={r.tomorrow} onChange={(e) => setR({ ...r, tomorrow: e.target.value })} />
              <button
                type="button"
                className="pl-m-btn"
                onClick={async () => {
                  const reward = await mentorIpc.review(r.wentWell, r.learned, r.tomorrow);
                  setReview(false);
                  setR({ wentWell: "", learned: "", tomorrow: "" });
                  showToast(`Итог записан: +${reward.xp} XP${r.tomorrow.trim() ? ", главное на завтра — в задачах" : ""}`);
                }}
              >
                Сохранить итог
              </button>
            </>
          ) : (
            <p className="pl-m-note">Три вопроса на две минуты. Вспомнить выученное — лучший способ его не забыть.</p>
          )}
        </div>
      )}
    </aside>
  );
}
