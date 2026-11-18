// Reigne-inspired 52-week emerald activity & streak heatmap (Reference 1: `b54f2700.jpg`).
// Aggregates real daily Git commits, Focus minutes, completed Tasks, and Agent
// actions from `hub_activity_get` — zero synthetic numbers in production.

import { useEffect, useMemo, useState } from "react";
import { localDateIso, useHub } from "../../store/hub";
import type { ActivityDay } from "../../lib/types";

const MONTHS = ["Янв", "Фев", "Мар", "Апр", "Май", "Июн", "Июл", "Авг", "Сен", "Окт", "Ноя", "Дек"];
const MONTHS_GEN = ["янв.", "февр.", "марта", "апр.", "мая", "июня", "июля", "авг.", "сент.", "окт.", "нояб.", "дек."];

function pluralActions(n: number): string {
  const d = n % 10;
  const h = n % 100;
  if (d === 1 && h !== 11) return `${n} действие за год`;
  if (d >= 2 && d <= 4 && (h < 12 || h > 14)) return `${n} действия за год`;
  return `${n} действий за год`;
}

function formatDayDetail(d: ActivityDay): string {
  const parts = d.date.split("-").map(Number);
  const mIdx = (parts[1] ?? 1) - 1;
  const dayNum = parts[2] ?? 1;
  const dateLabel = `${dayNum} ${MONTHS_GEN[mIdx] ?? ""}`;
  if (d.count === 0) return `${dateLabel} · Нет активности`;
  const breakdown: string[] = [];
  if (d.gitCommits > 0) breakdown.push(`${d.gitCommits} комм.`);
  if (d.focusMinutes > 0) breakdown.push(`${d.focusMinutes} мин фокуса`);
  if (d.tasksDone > 0) breakdown.push(`${d.tasksDone} зад.`);
  if (d.agentActions > 0) breakdown.push(`${d.agentActions} шаг. агента`);
  return `${dateLabel} · ${d.count}: ${breakdown.join(" · ") || "активность"}`;
}

interface GridCell {
  iso: string;
  future: boolean;
  isToday: boolean;
  data: ActivityDay;
}

export function ActivityDeck() {
  const activity = useHub((s) => s.activity);
  const refreshActivity = useHub((s) => s.refreshActivity);
  const [hovered, setHovered] = useState<ActivityDay | null>(null);

  useEffect(() => {
    void refreshActivity();
  }, [refreshActivity]);

  const { weeks, monthTicks } = useMemo(() => {
    const byDate = new Map<string, ActivityDay>();
    for (const d of activity?.days ?? []) {
      byDate.set(d.date, d);
    }

    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const todayIso = localDateIso(today);

    // Align the 52nd (rightmost) column to the current Monday..Sunday week.
    const jsDay = today.getDay();
    const diffToMon = jsDay === 0 ? -6 : 1 - jsDay;
    const currentMon = new Date(today.getFullYear(), today.getMonth(), today.getDate() + diffToMon);
    const startMon = new Date(currentMon.getFullYear(), currentMon.getMonth(), currentMon.getDate() - 51 * 7);

    const cols: GridCell[][] = [];
    const rawTransitions: Array<{ col: number; label: string }> = [];
    let lastMonth = -1;

    for (let w = 0; w < 52; w++) {
      const col: GridCell[] = [];
      for (let r = 0; r < 7; r++) {
        const dt = new Date(startMon.getFullYear(), startMon.getMonth(), startMon.getDate() + w * 7 + r);
        const iso = localDateIso(dt);
        const future = dt.getTime() > today.getTime();
        if (r === 0) {
          const m = dt.getMonth();
          if (m !== lastMonth) {
            lastMonth = m;
            rawTransitions.push({ col: w, label: MONTHS[m] ?? "" });
          }
        }
        const found = byDate.get(iso);
        col.push({
          iso,
          future,
          isToday: iso === todayIso,
          data: found ?? {
            date: iso,
            count: 0,
            level: 0,
            focusMinutes: 0,
            tasksDone: 0,
            agentActions: 0,
            gitCommits: 0,
          },
        });
      }
      cols.push(col);
    }

    // Drop a trailing 1–2 week partial month stub at column 0 so all 12 full months
    // (`Дек .. Ноя`) appear cleanly without colliding at the left edge (Ref 1).
    const candidates =
      rawTransitions.length >= 2 && rawTransitions[0]!.col === 0 && rawTransitions[1]!.col <= 3
        ? rawTransitions.slice(1)
        : rawTransitions;

    const ticks: Array<{ col: number; label: string }> = [];
    let lastTickPx = -999;
    for (const c of candidates) {
      const px = c.col * 10;
      if (px - lastTickPx >= 26) {
        ticks.push(c);
        lastTickPx = px;
      }
    }

    return { weeks: cols, monthTicks: ticks };
  }, [activity]);

  const userName = activity?.userName?.trim() || "Пользователь";
  const initial = userName.charAt(0).toUpperCase() || "J";
  const totalYear = activity?.totalYear ?? 0;
  const currentStreak = activity?.currentStreak ?? 0;
  const longestStreak = activity?.longestStreak ?? 0;

  return (
    <div className="activity-deck">
      <div className="activity-head">
        <div className="activity-user">
          <span className="activity-avatar" aria-hidden="true">
            {initial}
          </span>
          <span className="activity-username">{userName}</span>
        </div>
        <span className="activity-total">{pluralActions(totalYear)}</span>
      </div>

      <div className="activity-matrix-wrap" onMouseLeave={() => setHovered(null)}>
        <div className="activity-months" aria-hidden="true">
          {monthTicks.map((t) => (
            <span key={`${t.col}-${t.label}`} className="activity-month" style={{ left: `${t.col * 10}px` }}>
              {t.label}
            </span>
          ))}
        </div>

        <div className="activity-grid" role="img" aria-label={`График активности: ${pluralActions(totalYear)}`}>
          {weeks.map((col, wIdx) => (
            <div key={wIdx} className="activity-col">
              {col.map((cell) => {
                const lvl = Math.min(4, Math.max(0, cell.data.level));
                return (
                  <span
                    key={cell.iso}
                    className={`activity-cell lvl-${cell.future ? "future" : lvl}${cell.isToday ? " is-today" : ""}`}
                    onMouseEnter={() => {
                      if (!cell.future) setHovered(cell.data);
                    }}
                  />
                );
              })}
            </div>
          ))}
        </div>
      </div>

      <div className="activity-foot">
        {hovered ? (
          <span className="activity-hover-detail">{formatDayDetail(hovered)}</span>
        ) : (
          <div className="activity-streaks">
            <span>
              Текущая серия <strong>{currentStreak} дн.</strong>
            </span>
            <span className="activity-sep">·</span>
            <span>
              Рекорд <strong>{longestStreak} дн.</strong>
            </span>
          </div>
        )}

        <div className="activity-legend" aria-hidden="true">
          <span>Меньше</span>
          <i className="activity-cell lvl-0" />
          <i className="activity-cell lvl-1" />
          <i className="activity-cell lvl-2" />
          <i className="activity-cell lvl-3" />
          <i className="activity-cell lvl-4" />
          <span>Больше</span>
        </div>
      </div>
    </div>
  );
}
