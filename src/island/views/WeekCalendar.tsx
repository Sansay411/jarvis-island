// The week on the overview deck: a 7-day strip and the selected day — the
// planner's events (what is on now, with its progress, and what is next),
// tasks and reminders in one list by time. A block opens in the planner;
// one on now or next starts focus right here.

import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../ui/Icon";
import { isTauri } from "../../lib/ipc";
import { localDateIso, useHub } from "../../store/hub";
import { reminderCancel, reminderDone, useReminders } from "../../store/reminders";
import { useTick } from "../../store/stopwatch";
import { dayRows, eventsOn, focusable, inLabel, useAgenda, type AgendaRow } from "../../store/agenda";
import { durationLabel, parseLocal } from "../../planner/dates";
import { emote } from "../../store/emote";

const MONTHS_SHORT = ["Январь", "Февраль", "Март", "Апрель", "Май", "Июнь", "Июль", "Август", "Сентябрь", "Октябрь", "Ноябрь", "Декабрь"];
const WEEKDAYS_SHORT = ["ПН", "ВТ", "СР", "ЧТ", "ПТ", "СБ", "ВС"];

function startOfIsoWeek(base: Date, weekOffset: number): Date {
  const d = new Date(base.getFullYear(), base.getMonth(), base.getDate());
  const jsDay = d.getDay(); // 0=Sun..6=Sat
  const diffToMon = jsDay === 0 ? -6 : 1 - jsDay;
  d.setDate(d.getDate() + diffToMon + weekOffset * 7);
  return d;
}

const toFocus = (ms: number) => Math.min(240, Math.max(5, Math.round(ms / 300_000) * 5));

function EventRow({ row, now }: { row: Extract<AgendaRow, { kind: "event" }>; now: Date }) {
  const focusActive = useHub((s) => s.focus.active);
  const focusStart = useHub((s) => s.focusStart);
  const e = row.event;
  const t = now.getTime();
  const start = parseLocal(e.start).getTime();
  const end = parseLocal(e.end).getTime();
  const live = row.state === "now";
  const toStart = Math.ceil((start - t) / 60_000);
  const label = row.state === "allday"
    ? "весь день"
    : live
      ? `ещё ${durationLabel(Math.max(1, Math.ceil((end - t) / 60_000)))}`
      : row.state === "next" && toStart <= 180
        ? inLabel(toStart)
        : row.time;
  const canFocus = (live || row.state === "next") && focusable(e) && !focusActive && !e.done;
  const progress = live ? Math.min(100, Math.max(0, ((t - start) / Math.max(1, end - start)) * 100)) : 0;
  return (
    <div
      data-key={row.key}
      className={`agenda-row is-event is-${row.state}${e.done ? " is-done" : ""}`}
      style={live ? { ["--p" as string]: `${progress.toFixed(1)}%` } : undefined}
    >
      <span className="agenda-mark" aria-hidden="true" />
      <button
        type="button"
        className="agenda-title"
        title={`${e.title}${e.location ? ` · ${e.location}` : ""} — открыть в планировщике`}
        onClick={() => isTauri() && void invoke("planner_open", { date: e.start.slice(0, 10) })}
      >
        {e.title}
      </button>
      <span className="agenda-time">{label}</span>
      {canFocus && (
        <button
          type="button"
          className="agenda-go"
          aria-label="Начать фокус"
          title={live ? "Фокус до конца блока" : "Фокус на весь блок"}
          onClick={() => {
            emote("determined");
            void focusStart(e.title, toFocus(end - Math.max(start, t)), null);
          }}
        >
          <Icon name="play" size={9} />
        </button>
      )}
    </div>
  );
}

export function WeekCalendar() {
  useTick(30_000);
  const now = new Date();
  const todayIso = localDateIso(now);
  const [selectedIso, setSelectedIso] = useState<string>(todayIso);
  const [weekOffset, setWeekOffset] = useState<number>(0);

  const tasks = useHub((s) => s.tasks);
  const taskToggle = useHub((s) => s.taskToggle);
  const reminders = useReminders((s) => s.list);
  const events = useAgenda((s) => s.events);

  const weekDays = useMemo(() => {
    const mon = startOfIsoWeek(new Date(), weekOffset);
    return Array.from({ length: 7 }, (_, i) => {
      const dt = new Date(mon.getFullYear(), mon.getMonth(), mon.getDate() + i);
      const iso = localDateIso(dt);
      const hasItems =
        eventsOn(events, iso).length > 0 ||
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
  }, [weekOffset, events, tasks, reminders, todayIso]);

  const activeDayObj = weekDays.find((d) => d.iso === selectedIso) ?? weekDays[3]!;
  const monthLabel = MONTHS_SHORT[activeDayObj.monthIdx] ?? "";
  const rows = dayRows(events, tasks, reminders, selectedIso, now);

  // Today opens on what is on now (or next) when the morning pushed it out of sight.
  const listRef = useRef<HTMLDivElement>(null);
  const anchor = selectedIso === todayIso ? rows.find((r) => r.kind === "event" && (r.state === "now" || r.state === "next"))?.key ?? null : null;
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const el = anchor ? list.querySelector<HTMLElement>(`[data-key="${anchor}"]`) : null;
    if (!el) {
      list.scrollTop = 0;
      return;
    }
    const top = el.getBoundingClientRect().top - list.getBoundingClientRect().top + list.scrollTop;
    if (top + el.offsetHeight > list.clientHeight) list.scrollTop = Math.max(0, top - 2);
  }, [anchor, selectedIso]);

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

      <div ref={listRef} className="week-agenda scroll">
        {rows.length === 0 ? (
          <div className="week-empty">
            <Icon name="calendar" size={14} />
            <span>{selectedIso === todayIso ? "Сегодня свободно" : "На этот день ничего нет"}</span>
          </div>
        ) : (
          rows.map((row) =>
            row.kind === "event" ? (
              <EventRow key={row.key} row={row} now={now} />
            ) : row.kind === "task" ? (
              <div key={row.key} className={`agenda-row${row.task.completed ? " is-done" : ""}`}>
                <button
                  type="button"
                  className={`task-ring ring-${row.task.color || "blue"}${row.task.completed ? " is-checked" : ""}`}
                  aria-label={row.task.completed ? "Вернуть задачу" : "Отметить выполненной"}
                  onClick={() => void taskToggle(row.task.id)}
                >
                  {row.task.completed && <Icon name="check" size={10} />}
                </button>
                <span className="agenda-title" title={row.task.title}>
                  {row.task.title}
                </span>
                {row.time && <span className="agenda-time">{row.time}</span>}
              </div>
            ) : (
              <div key={row.key} className="agenda-row">
                <button
                  type="button"
                  className="task-ring ring-amber"
                  aria-label={row.reminder.repeatLabel ? "Отменить серию напоминаний" : "Завершить напоминание"}
                  title={row.reminder.repeatLabel ? `Отменить серию (${row.reminder.repeatLabel})` : "Завершить напоминание"}
                  onClick={() => void (row.reminder.repeatLabel ? reminderCancel(row.reminder.id) : reminderDone(row.reminder.id))}
                >
                  <Icon name={row.reminder.repeatLabel ? "close" : "check"} size={10} />
                </button>
                <span className="agenda-title" title={row.reminder.text}>
                  {row.reminder.text || "Напоминание"}
                </span>
                {row.reminder.repeatLabel && (
                  <span className="agenda-repeat" title={`${row.reminder.repeatLabel} · след. ${row.time}`}>
                    <Icon name="restart" size={10} />
                    <span>{row.reminder.repeatLabel}</span>
                  </span>
                )}
                <span className="agenda-time">{row.time}</span>
              </div>
            ),
          )
        )}
      </div>
    </div>
  );
}
