// The small month in the sidebar: today in a blue circle, the days on
// screen tinted, a click jumps there.

import { useEffect, useState } from "react";
import { addDays, isoDate, MONTHS, sameDay, startOfDay, startOfMonth, startOfWeek, WEEKDAYS_MINI } from "./dates";
import { PIcon } from "./icons";

export function MiniMonth({ anchor, now, rangeStart, rangeDays, onPick }: { anchor: Date; now: Date; rangeStart: Date; rangeDays: number; onPick: (d: Date) => void }) {
  const [month, setMonth] = useState(() => startOfMonth(anchor));
  useEffect(() => setMonth(startOfMonth(anchor)), [anchor]);
  const first = startOfWeek(month);
  const rangeEnd = addDays(rangeStart, rangeDays);
  const title = `${MONTHS[month.getMonth()]!.charAt(0).toUpperCase()}${MONTHS[month.getMonth()]!.slice(1)} ${month.getFullYear()}`;
  return (
    <div className="pl-mini">
      <div className="pl-mini-head">
        <span className="pl-mini-title">{title}</span>
        <button type="button" className="pl-icon-btn is-sm" aria-label="Предыдущий месяц" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))}>
          <PIcon name="left" size={18} />
        </button>
        <button type="button" className="pl-icon-btn is-sm" aria-label="Следующий месяц" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))}>
          <PIcon name="right" size={18} />
        </button>
      </div>
      <div className="pl-mini-grid">
        {WEEKDAYS_MINI.map((w, i) => (
          <span key={i} className="pl-mini-dow">
            {w}
          </span>
        ))}
        {Array.from({ length: 42 }, (_, i) => {
          const d = addDays(first, i);
          const inRange = d >= startOfDay(rangeStart) && d < rangeEnd && rangeDays < 42;
          const cls = [
            "pl-mini-day",
            d.getMonth() !== month.getMonth() ? "is-outside" : "",
            sameDay(d, now) ? "is-today" : "",
            sameDay(d, anchor) ? "is-anchor" : "",
            inRange ? "is-range" : "",
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <button key={isoDate(d)} type="button" className={cls} onClick={() => onPick(d)} aria-label={isoDate(d)}>
              {d.getDate()}
            </button>
          );
        })}
      </div>
    </div>
  );
}
