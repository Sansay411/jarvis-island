// The week on the overview deck: a 7-day strip and the selected day's tasks
// and reminders. Read-and-tick only — tasks are added in «Фокус» or by asking.

import { useMemo, useState } from "react";
import { Icon } from "../../ui/Icon";
import { localDateIso, useHub } from "../../store/hub";
import { localTime, reminderDone, useReminders } from "../../store/reminders";

const MONTHS_SHORT = ["Январь", "Февраль", "Март", "Апрель", "Май", "Июнь", "Июль", "Август", "Сентябрь", "Октябрь", "Ноябрь", "Декабрь"];
const WEEKDAYS_SHORT = ["ПН", "ВТ", "СР", "ЧТ", "ПТ", "СБ", "ВС"];

function startOfIsoWeek(base: Date, weekOffset: number): Date {
  const d = new Date(base.getFullYear(), base.getMonth(), base.getDate());
  const jsDay = d.getDay(); // 0=Sun..6=Sat
  const diffToMon = jsDay === 0 ? -6 : 1 - jsDay;
  d.setDate(d.getDate() + diffToMon + weekOffset * 7);
  return d;
}

export function WeekCalendar() {
  const todayIso = localDateIso();
  const [selectedIso, setSelectedIso] = useState<string>(todayIso);
  const [weekOffset, setWeekOffset] = useState<number>(0);

  const tasks = useHub((s) => s.tasks);
  const taskToggle = useHub((s) => s.taskToggle);
  const reminders = useReminders((s) => s.list);

  const weekDays = useMemo(() => {
    const mon = startOfIsoWeek(new Date(), weekOffset);
    return Array.from({ length: 7 }, (_, i) => {
      const dt = new Date(mon.getFullYear(), mon.getMonth(), mon.getDate() + i);
      const iso = localDateIso(dt);
      const hasItems =
        tasks.some((t) => !t.completed && (t.scheduledDate === iso || (!t.scheduledDate && iso === todayIso))) ||
        reminders.some((r) => !r.done && r.kind === "reminder" && localDateIso(new Date(r.dueAt)) === iso);
      return {
        iso,
        dow: WEEKDAYS_SHORT[i]!,
        dayNum: dt.getDate(),
        monthIdx: dt.getMonth(),
        isToday: iso === todayIso,
        hasItems,
      };
    });
  }, [weekOffset, tasks, reminders, todayIso]);

  const activeDayObj = weekDays.find((d) => d.iso === selectedIso) ?? weekDays[3]!;
  const monthLabel = MONTHS_SHORT[activeDayObj.monthIdx] ?? "";

  const dayTasks = useMemo(() => {
    return tasks
      .filter((t) => {
        if (t.scheduledDate) return t.scheduledDate === selectedIso;
        return selectedIso === todayIso && !t.completed;
      })
      .sort((a, b) => {
        if (a.completed !== b.completed) return a.completed ? 1 : -1;
        return (a.scheduledTime ?? "99:99").localeCompare(b.scheduledTime ?? "99:99");
      });
  }, [tasks, selectedIso, todayIso]);

  const dayReminders = useMemo(() => {
    return reminders.filter(
      (r) => !r.done && r.kind === "reminder" && localDateIso(new Date(r.dueAt)) === selectedIso,
    );
  }, [reminders, selectedIso]);

  return (
    <div className="week-cal">
      <div className="week-cal-head">
        <button
          type="button"
          className="week-month-btn"
          title="Вернуться к сегодня"
          onClick={() => {
            setWeekOffset(0);
            setSelectedIso(todayIso);
          }}
        >
          {monthLabel}
        </button>

        <div className="week-strip" role="tablist" aria-label="Дни недели">
          <button
            type="button"
            className="week-nav-btn"
            aria-label="Предыдущая неделя"
            onClick={() => setWeekOffset((w) => w - 1)}
          >
            <Icon name="chevronLeft" size={11} />
          </button>

          {weekDays.map((d) => {
            const selected = d.iso === selectedIso;
            return (
              <button
                key={d.iso}
                type="button"
                role="tab"
                aria-selected={selected}
                className={`week-day${selected ? " is-selected" : ""}${d.isToday ? " is-today" : ""}`}
                onClick={() => setSelectedIso(d.iso)}
              >
                <span className="week-dow">{d.dow}</span>
                <span className="week-num">{d.dayNum}</span>
                {d.hasItems && <span className="week-dot" aria-hidden="true" />}
              </button>
            );
          })}

          <button
            type="button"
            className="week-nav-btn"
            aria-label="Следующая неделя"
            onClick={() => setWeekOffset((w) => w + 1)}
          >
            <Icon name="chevron" size={11} />
          </button>
        </div>
      </div>

      <div className="week-agenda scroll">
        {dayTasks.length === 0 && dayReminders.length === 0 ? (
          <div className="week-empty">
            <Icon name="calendar" size={14} />
            <span>На этот день задач нет</span>
          </div>
        ) : (
          <>
            {dayTasks.map((t) => (
              <div key={t.id} className={`agenda-row${t.completed ? " is-done" : ""}`}>
                <button
                  type="button"
                  className={`task-ring ring-${t.color || "blue"}${t.completed ? " is-checked" : ""}`}
                  aria-label={t.completed ? "Вернуть задачу" : "Отметить выполненной"}
                  onClick={() => void taskToggle(t.id)}
                >
                  {t.completed && <Icon name="check" size={10} />}
                </button>
                <span className="agenda-title" title={t.title}>
                  {t.title}
                </span>
                {t.scheduledTime && <span className="agenda-time">{t.scheduledTime}</span>}
              </div>
            ))}
            {dayReminders.map((r) => (
              <div key={`rem-${r.id}`} className="agenda-row">
                <button
                  type="button"
                  className="task-ring ring-amber"
                  aria-label="Завершить напоминание"
                  onClick={() => void reminderDone(r.id)}
                >
                  <Icon name="check" size={10} />
                </button>
                <span className="agenda-title" title={r.text}>
                  {r.text || "Напоминание"}
                </span>
                <span className="agenda-time">{localTime(r.dueAt)}</span>
              </div>
            ))}
          </>
        )}
      </div>

    </div>
  );
}
