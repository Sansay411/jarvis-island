// Day, 4-day and week views: Google Calendar's hour grid. 48 px an hour,
// hairlines on the hours, a red «now» line in today's column, events
// packed side by side. Drag empty space to create, drag an event to move it
// (across days too), drag its bottom edge to stretch it — all on a
// 15-minute grid. All-day and multi-day events sit in a row under the dates.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as RPointerEvent } from "react";
import type { TaskItem } from "../lib/types";
import { eventColor, eventHex, hexOf, inkOn, categoryOf, type CalEvent, type Suggestion } from "../store/planner";
import { addDays, addMinutes, dayDiff, gmtLabel, hm, isoDate, minutesOfDay, parseLocal, sameDay, startOfDay, timeLabel, WEEKDAYS_SHORT, weekdayIndex } from "./dates";
import { isBanner, layoutBanners, layoutDay } from "./layout";

export const HOUR_H = 48;
const PX_PER_MIN = HOUR_H / 60;
const BANNER_H = 24;
const SNAP = 15;

export interface Draft {
  start: Date;
  end: Date;
  allDay: boolean;
}

interface Props {
  days: Date[];
  events: CalEvent[];
  tasks: TaskItem[];
  suggestions: Suggestion[];
  now: Date;
  scrollToHour: number;
  selectedId: string | null;
  draft: Draft | null;
  onCreate: (d: Draft, anchor: DOMRect) => void;
  onOpen: (e: CalEvent, anchor: DOMRect) => void;
  onEdit: (e: CalEvent) => void;
  onMove: (e: CalEvent, start: Date, end: Date, allDay: boolean) => void;
  onDay: (d: Date) => void;
  onTask: (t: TaskItem) => void;
  onAccept: (s: Suggestion) => void;
}

type Drag =
  | { kind: "create"; day: number; from: number; to: number; moved: boolean; x: number; y: number }
  | { kind: "move"; ev: CalEvent; grab: number; dur: number; day: number; start: number; moved: boolean; x: number; y: number; el: HTMLElement }
  | { kind: "resize"; ev: CalEvent; day: number; start: number; end: number; moved: boolean; x: number; y: number };

const clamp = (v: number, a: number, b: number) => Math.max(a, Math.min(b, v));

export function TimeGrid(p: Props) {
  const { days, now } = p;
  const scrollRef = useRef<HTMLDivElement>(null);
  const colsRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  dragRef.current = drag;

  // Open on the working morning (or a bit above «now» when it is later).
  const scrolled = useRef(false);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || scrolled.current) return;
    scrolled.current = true;
    const showsToday = days.some((d) => sameDay(d, now));
    const nowH = now.getHours() + now.getMinutes() / 60;
    const visibleH = el.clientHeight / HOUR_H;
    // The working morning, unless «now» would fall below the fold.
    const hour = showsToday && nowH > p.scrollToHour + visibleH - 1.5 ? nowH - visibleH / 3 : p.scrollToHour;
    el.scrollTop = Math.max(0, hour * HOUR_H - 8);
  }, [days, now, p.scrollToHour]);

  const timed = useMemo(() => p.events.filter((e) => !isBanner(e)), [p.events]);
  const banners = useMemo(() => layoutBanners(p.events, days[0]!, days.length), [p.events, days]);
  const placed = useMemo(() => days.map((d) => layoutDay(timed, d)), [timed, days]);
  const dayTasks = useMemo(
    () => days.map((d) => p.tasks.filter((t) => t.scheduledDate === isoDate(d))),
    [p.tasks, days],
  );
  const untimedTaskRows = dayTasks.map((ts) => ts.filter((t) => !t.scheduledTime));
  /** Banner rows taken on column `i`; untimed tasks stack under them. */
  const rowsOn = (i: number) => banners.filter((b) => b.from <= i && b.to >= i).reduce((m, b) => Math.max(m, b.row + 1), 0);
  const allDayRows = Math.max(
    banners.reduce((m, b) => Math.max(m, b.row + 1), 0),
    ...untimedTaskRows.map((ts, i) => ts.length + rowsOn(i)),
  );

  const pos = (clientX: number, clientY: number) => {
    const r = colsRef.current!.getBoundingClientRect();
    const x = clamp(clientX - r.left, 0, r.width - 1);
    const y = clamp(clientY - r.top, 0, r.height - 1);
    return { day: clamp(Math.floor(x / (r.width / days.length)), 0, days.length - 1), min: y / PX_PER_MIN };
  };

  // Keep scrolling while a drag holds the pointer at the top or bottom edge.
  const edgeRaf = useRef(0);
  const lastPointer = useRef<{ x: number; y: number } | null>(null);
  useEffect(() => {
    if (!drag) return;
    const tick = () => {
      const el = scrollRef.current;
      const pt = lastPointer.current;
      if (el && pt) {
        const r = el.getBoundingClientRect();
        const dy = pt.y < r.top + 36 ? -12 : pt.y > r.bottom - 36 ? 12 : 0;
        if (dy) {
          el.scrollTop += dy;
          onMoveAt(pt.x, pt.y);
        }
      }
      edgeRaf.current = requestAnimationFrame(tick);
    };
    edgeRaf.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(edgeRaf.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drag?.kind]);

  const onMoveAt = (clientX: number, clientY: number) => {
    const d = dragRef.current;
    if (!d) return;
    const moved = d.moved || Math.abs(clientX - d.x) > 4 || Math.abs(clientY - d.y) > 4;
    const at = pos(clientX, clientY);
    if (d.kind === "create") {
      setDrag({ ...d, moved, day: d.day, to: clamp(Math.ceil(at.min / SNAP) * SNAP, 0, 1440) });
    } else if (d.kind === "move") {
      const start = clamp(Math.round((at.min - d.grab) / SNAP) * SNAP, 0, 1440 - SNAP);
      setDrag({ ...d, moved, day: at.day, start });
    } else {
      setDrag({ ...d, moved, end: clamp(Math.round(at.min / SNAP) * SNAP, d.start + SNAP, 1440) });
    }
  };

  const onColsDown = (e: RPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest(".pl-ev, .pl-task, .pl-sug")) return;
    const at = pos(e.clientX, e.clientY);
    const from = Math.floor(at.min / SNAP) * SNAP;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    lastPointer.current = { x: e.clientX, y: e.clientY };
    setDrag({ kind: "create", day: at.day, from, to: from + SNAP, moved: false, x: e.clientX, y: e.clientY });
  };

  const onEvDown = (e: RPointerEvent<HTMLElement>, ev: CalEvent, resize: boolean) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const at = pos(e.clientX, e.clientY);
    const s = parseLocal(ev.start);
    const en = parseLocal(ev.end);
    const day0 = startOfDay(days[0]!);
    const startDay = clamp(dayDiff(day0, s), 0, days.length - 1);
    const startMin = dayDiff(day0, s) === startDay ? minutesOfDay(s) : 0;
    colsRef.current!.setPointerCapture(e.pointerId);
    lastPointer.current = { x: e.clientX, y: e.clientY };
    if (resize) {
      const endMin = sameDay(s, en) ? minutesOfDay(en) : 1440;
      setDrag({ kind: "resize", ev, day: startDay, start: startMin, end: endMin, moved: false, x: e.clientX, y: e.clientY });
    } else {
      const dur = Math.max(SNAP, (en.getTime() - s.getTime()) / 60_000);
      // Grab offset within the event as it appears in the column under the pointer.
      const colStart = at.day === startDay ? startMin : 0;
      setDrag({ kind: "move", ev, grab: at.min - colStart, dur, day: startDay, start: startMin, moved: false, x: e.clientX, y: e.clientY, el: e.currentTarget as HTMLElement });
    }
  };

  const onColsMove = (e: RPointerEvent<HTMLDivElement>) => {
    if (!dragRef.current) return;
    lastPointer.current = { x: e.clientX, y: e.clientY };
    onMoveAt(e.clientX, e.clientY);
  };

  const onColsUp = (e: RPointerEvent<HTMLDivElement>) => {
    const d = dragRef.current;
    lastPointer.current = null;
    setDrag(null);
    if (!d) return;
    const base = startOfDay(days[d.day]!);
    if (d.kind === "create") {
      let from = d.from;
      let to = d.to;
      if (!d.moved) {
        // A click: one hour from the half hour under the pointer.
        from = Math.floor(d.from / 30) * 30;
        to = from + 60;
      } else if (to < from) [from, to] = [to, from + SNAP];
      if (to - from < SNAP) to = from + SNAP;
      const start = addMinutes(base, from);
      const end = addMinutes(base, Math.min(to, 1440));
      const col = colsRef.current!.querySelectorAll<HTMLElement>(".pl-col")[d.day];
      const r = col?.getBoundingClientRect() ?? new DOMRect(e.clientX, e.clientY, 0, 0);
      const top = r.top + from * PX_PER_MIN;
      p.onCreate({ start, end, allDay: false }, new DOMRect(r.left, top, r.width, Math.max(24, (to - from) * PX_PER_MIN)));
      return;
    }
    if (!d.moved) {
      if (d.kind === "move") p.onOpen(d.ev, d.el.getBoundingClientRect());
      return;
    }
    if (d.kind === "move") {
      const start = addMinutes(base, d.start);
      p.onMove(d.ev, start, addMinutes(start, d.dur), false);
    } else {
      const s = parseLocal(d.ev.start);
      p.onMove(d.ev, s, addMinutes(startOfDay(days[d.day]!), d.end), false);
    }
  };

  // Drag in the all-day row: create on a click, move bars between days.
  const alldayRef = useRef<HTMLDivElement>(null);
  const [bannerDrag, setBannerDrag] = useState<{ ev: CalEvent | null; fromDay: number; day: number; moved: boolean; x: number } | null>(null);
  const dayAt = (clientX: number) => {
    const r = alldayRef.current!.getBoundingClientRect();
    return clamp(Math.floor((clientX - r.left) / (r.width / days.length)), 0, days.length - 1);
  };

  const nowTop = minutesOfDay(now) * PX_PER_MIN;
  const ghost = drag && drag.moved ? drag : drag?.kind === "create" ? drag : null;

  return (
    <div className={`pl-grid${drag ? " is-dragging" : ""}${days.length === 1 ? " is-single" : ""}`} style={{ ["--days" as string]: days.length }}>
      <div className="pl-heads">
        <div className="pl-gutter-head" />
        {days.map((d) => {
          const today = sameDay(d, now);
          return (
            <div key={isoDate(d)} className={`pl-dayhead${today ? " is-today" : ""}${d < startOfDay(now) ? " is-past" : ""}`}>
              <span className="pl-dow">{WEEKDAYS_SHORT[weekdayIndex(d)]}</span>
              <button type="button" className="pl-date" onClick={() => p.onDay(d)} aria-label={`Открыть ${d.getDate()}`}>
                {d.getDate()}
              </button>
            </div>
          );
        })}
      </div>

      <div className="pl-allday-row">
        <div className="pl-gmt">{gmtLabel(now)}</div>
        <div
          ref={alldayRef}
          className="pl-allday"
          style={{ height: Math.max(1, allDayRows) * BANNER_H + 8 }}
          onPointerDown={(e) => {
            if (e.button !== 0) return;
            const el = (e.target as HTMLElement).closest<HTMLElement>("[data-banner]");
            const ev = el ? p.events.find((x) => x.id === el.dataset.banner) ?? null : null;
            if ((e.target as HTMLElement).closest(".pl-task")) return;
            (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
            const day = dayAt(e.clientX);
            setBannerDrag({ ev, fromDay: day, day, moved: false, x: e.clientX });
          }}
          onPointerMove={(e) => {
            if (!bannerDrag) return;
            const day = dayAt(e.clientX);
            setBannerDrag({ ...bannerDrag, day, moved: bannerDrag.moved || Math.abs(e.clientX - bannerDrag.x) > 4 });
          }}
          onPointerUp={(e) => {
            const b = bannerDrag;
            setBannerDrag(null);
            if (!b) return;
            if (b.ev && !b.moved) {
              const el = alldayRef.current!.querySelector<HTMLElement>(`[data-banner="${CSS.escape(b.ev.id)}"]`);
              p.onOpen(b.ev, el?.getBoundingClientRect() ?? new DOMRect(e.clientX, e.clientY, 0, 0));
              return;
            }
            if (b.ev) {
              const shift = b.day - b.fromDay;
              if (shift !== 0) {
                const s = addDays(parseLocal(b.ev.start), shift);
                const en = addDays(parseLocal(b.ev.end), shift);
                p.onMove(b.ev, s, en, b.ev.allDay);
              }
              return;
            }
            const a = Math.min(b.fromDay, b.day);
            const z = Math.max(b.fromDay, b.day);
            const r = alldayRef.current!.getBoundingClientRect();
            const w = r.width / days.length;
            p.onCreate({ start: startOfDay(days[a]!), end: addDays(startOfDay(days[z]!), 1), allDay: true }, new DOMRect(r.left + a * w, r.top, (z - a + 1) * w, r.height));
          }}
        >
          {days.map((_, i) => (
            <div key={i} className="pl-allday-col" style={{ left: `${(i / days.length) * 100}%`, width: `${100 / days.length}%` }} />
          ))}
          {banners.map((b) => {
            const shift = bannerDrag?.ev?.id === b.item.id && bannerDrag.moved ? bannerDrag.day - bannerDrag.fromDay : 0;
            const from = clamp(b.from + shift, 0, days.length - 1);
            const to = clamp(b.to + shift, 0, days.length - 1);
            const color = eventColor(b.item);
            return (
              <div
                key={b.item.id}
                data-banner={b.item.id}
                className={`pl-banner${b.item.done ? " is-done" : ""}${p.selectedId === b.item.id ? " is-selected" : ""}${b.clippedStart ? " cut-start" : ""}${b.clippedEnd ? " cut-end" : ""}`}
                style={{
                  left: `calc(${(from / days.length) * 100}% + 2px)`,
                  width: `calc(${((to - from + 1) / days.length) * 100}% - 10px)`,
                  top: b.row * BANNER_H + 2,
                  background: hexOf(color),
                  color: inkOn(color),
                }}
                title={b.item.title}
              >
                <span className="pl-banner-title">{b.item.title}</span>
              </div>
            );
          })}
          {untimedTaskRows.map((ts, i) => {
            const taken = rowsOn(i);
            return ts.map((t, k) => (
              <button
                key={t.id}
                type="button"
                className={`pl-task pl-task-banner${t.completed ? " is-done" : ""}`}
                style={{ left: `calc(${(i / days.length) * 100}% + 2px)`, width: `calc(${100 / days.length}% - 10px)`, top: (taken + k) * BANNER_H + 2 }}
                onClick={() => p.onTask(t)}
                title={t.completed ? "Вернуть в работу" : "Отметить выполненной"}
              >
                <TaskMark done={t.completed} />
                <span>{t.title}</span>
              </button>
            ));
          })}
          {bannerDrag && !bannerDrag.ev && (
            <div
              className="pl-banner is-draft"
              style={{
                left: `calc(${(Math.min(bannerDrag.fromDay, bannerDrag.day) / days.length) * 100}% + 2px)`,
                width: `calc(${((Math.abs(bannerDrag.day - bannerDrag.fromDay) + 1) / days.length) * 100}% - 10px)`,
                top: 2,
              }}
            >
              (Без названия)
            </div>
          )}
          {p.draft?.allDay && (
            <div
              className="pl-banner is-draft"
              style={{
                left: `calc(${(clamp(dayDiff(days[0]!, p.draft.start), 0, days.length - 1) / days.length) * 100}% + 2px)`,
                width: `calc(${(Math.max(1, dayDiff(p.draft.start, p.draft.end)) / days.length) * 100}% - 10px)`,
                top: 2,
              }}
            >
              (Без названия)
            </div>
          )}
        </div>
      </div>

      <div className="pl-scroll" ref={scrollRef}>
        <div className="pl-body" style={{ height: 24 * HOUR_H }}>
          <div className="pl-gutter">
            {Array.from({ length: 24 }, (_, h) =>
              h === 0 ? null : (
                <div key={h} className="pl-hour" style={{ top: h * HOUR_H }}>
                  <span>{timeLabel(h * 60)}</span>
                </div>
              ),
            )}
          </div>
          <div
            ref={colsRef}
            className="pl-cols"
            onPointerDown={onColsDown}
            onPointerMove={onColsMove}
            onPointerUp={onColsUp}
            onPointerCancel={() => setDrag(null)}
          >
            {days.map((d, i) => {
              const today = sameDay(d, now);
              const sugs = p.suggestions.filter((s) => sameDay(parseLocal(s.start), d));
              return (
                <div key={isoDate(d)} className={`pl-col${today ? " is-today" : ""}`}>
                  <div className="pl-col-events">
                    {placed[i]!.map((pl) => {
                      const ev = pl.item;
                      const isDragged = drag && drag.kind !== "create" && drag.moved && drag.ev.id === ev.id;
                      return (
                        <EventBlock
                          key={`${ev.id}-${i}`}
                          ev={ev}
                          top={pl.top}
                          height={pl.height}
                          left={pl.left}
                          width={pl.width}
                          past={parseLocal(ev.end) < now}
                          faded={Boolean(isDragged)}
                          selected={p.selectedId === ev.id}
                          cutTop={pl.fromBefore}
                          cutBottom={pl.intoNext}
                          z={pl.z}
                          onDown={(e, resize) => onEvDown(e, ev, resize)}
                          onDouble={() => p.onEdit(ev)}
                        />
                      );
                    })}
                    {dayTasks[i]!
                      .filter((t) => t.scheduledTime)
                      .map((t) => {
                        const [h, m] = t.scheduledTime!.split(":").map(Number);
                        const top = ((h ?? 0) * 60 + (m ?? 0)) * PX_PER_MIN;
                        return (
                          <button
                            key={t.id}
                            type="button"
                            className={`pl-task pl-task-timed${t.completed ? " is-done" : ""}`}
                            style={{ top: top + 1, height: 30 * PX_PER_MIN - 2 }}
                            onPointerDown={(e) => e.stopPropagation()}
                            onClick={() => p.onTask(t)}
                            title={t.completed ? "Вернуть в работу" : "Отметить выполненной"}
                          >
                            <TaskMark done={t.completed} />
                            <span className="pl-task-title">{t.title}</span>
                            <span className="pl-task-time">{t.scheduledTime}</span>
                          </button>
                        );
                      })}
                    {sugs.map((s, k) => {
                      const a = parseLocal(s.start);
                      const b = parseLocal(s.end);
                      const color = categoryOf(s.category).color;
                      return (
                        <button
                          key={`${s.start}-${k}`}
                          type="button"
                          className="pl-sug"
                          style={{
                            top: minutesOfDay(a) * PX_PER_MIN + 1,
                            height: Math.max(20, ((b.getTime() - a.getTime()) / 60_000) * PX_PER_MIN - 2),
                            ["--c" as string]: hexOf(color),
                          }}
                          onPointerDown={(e) => e.stopPropagation()}
                          onClick={() => p.onAccept(s)}
                          title={`${s.why ? s.why + " · " : ""}Нажмите, чтобы добавить`}
                        >
                          <span className="pl-sug-title">{s.title}</span>
                          <span className="pl-sug-time">
                            {hm(a)} – {hm(b)}
                          </span>
                          <span className="pl-sug-add" aria-hidden="true">
                            +
                          </span>
                        </button>
                      );
                    })}
                    {p.draft && !p.draft.allDay && sameDay(p.draft.start, d) && (
                      <DraftBlock start={p.draft.start} end={p.draft.end} />
                    )}
                    {ghost && ghost.day === i && <Ghost drag={ghost} />}
                  </div>
                  {today && (
                    <div className="pl-now" style={{ top: nowTop }} aria-hidden="true">
                      <i />
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}

function TaskMark({ done }: { done: boolean }) {
  return (
    <svg className="pl-task-mark" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="8" cy="8" r="6.2" />
      {done && <path d="m5.2 8.2 1.9 1.9 3.8-4.1" />}
    </svg>
  );
}

function EventBlock(props: {
  ev: CalEvent;
  top: number;
  height: number;
  left: number;
  width: number;
  past: boolean;
  faded: boolean;
  selected: boolean;
  cutTop: boolean;
  cutBottom: boolean;
  z: number;
  onDown: (e: RPointerEvent<HTMLElement>, resize: boolean) => void;
  onDouble: () => void;
}) {
  const { ev } = props;
  const color = eventColor(ev);
  const s = parseLocal(ev.start);
  const e = parseLocal(ev.end);
  const px = Math.max(props.height * PX_PER_MIN - 2, 16);
  const compact = px < 38;
  return (
    <div
      className={`pl-ev${compact ? " is-compact" : ""}${props.past ? " is-past" : ""}${props.faded ? " is-faded" : ""}${props.selected ? " is-selected" : ""}${ev.done ? " is-done" : ""}`}
      style={{
        top: props.top * PX_PER_MIN + 1,
        height: px,
        left: `${props.left * 100}%`,
        width: `calc(${props.width * 100}% - 2px)`,
        background: hexOf(color),
        color: inkOn(color),
        zIndex: props.selected ? 30 : props.z,
        borderTopLeftRadius: props.cutTop ? 0 : undefined,
        borderTopRightRadius: props.cutTop ? 0 : undefined,
        borderBottomLeftRadius: props.cutBottom ? 0 : undefined,
        borderBottomRightRadius: props.cutBottom ? 0 : undefined,
      }}
      onPointerDown={(x) => props.onDown(x, false)}
      onDoubleClick={props.onDouble}
      role="button"
      tabIndex={0}
      aria-label={`${ev.title}, ${hm(s)} – ${hm(e)}`}
    >
      {compact ? (
        <span className="pl-ev-line">
          <b>{ev.title}</b>, {hm(s)}
        </span>
      ) : (
        <>
          <span className="pl-ev-title">{ev.title}</span>
          <span className="pl-ev-time">
            {hm(s)} – {hm(e)}
          </span>
          {px > 70 && ev.location && <span className="pl-ev-time">{ev.location}</span>}
        </>
      )}
      {!props.cutBottom && <span className="pl-ev-resize" onPointerDown={(x) => props.onDown(x, true)} />}
    </div>
  );
}

function DraftBlock({ start, end }: { start: Date; end: Date }) {
  const from = minutesOfDay(start);
  const to = sameDay(start, end) ? minutesOfDay(end) : 1440;
  return (
    <div className="pl-ev pl-ev-draft" style={{ top: from * PX_PER_MIN + 1, height: Math.max((to - from) * PX_PER_MIN - 2, 16), left: 0, width: "calc(100% - 2px)" }}>
      <span className="pl-ev-title">(Без названия)</span>
      <span className="pl-ev-time">
        {timeLabel(from)} – {timeLabel(to)}
      </span>
    </div>
  );
}

function Ghost({ drag }: { drag: Drag }) {
  if (drag.kind === "create") {
    const a = Math.min(drag.from, drag.to);
    const b = Math.max(drag.from + SNAP, drag.to);
    return (
      <div className="pl-ev pl-ev-draft" style={{ top: a * PX_PER_MIN + 1, height: Math.max((b - a) * PX_PER_MIN - 2, 16), left: 0, width: "calc(100% - 2px)" }}>
        <span className="pl-ev-title">(Без названия)</span>
        <span className="pl-ev-time">
          {timeLabel(a)} – {timeLabel(b)}
        </span>
      </div>
    );
  }
  const color = eventColor(drag.ev);
  const start = drag.start;
  const end = drag.kind === "move" ? Math.min(1440, drag.start + drag.dur) : drag.end;
  return (
    <div
      className="pl-ev is-ghost"
      style={{ top: start * PX_PER_MIN + 1, height: Math.max((end - start) * PX_PER_MIN - 2, 16), left: 0, width: "calc(100% - 2px)", background: eventHex(drag.ev), color: inkOn(color) }}
    >
      <span className="pl-ev-title">{drag.ev.title}</span>
      <span className="pl-ev-time">
        {timeLabel(start)} – {timeLabel(drag.kind === "move" ? drag.start + drag.dur : end)}
      </span>
    </div>
  );
}

export function useNow(frozen: Date | null): Date {
  const [now, setNow] = useState(() => frozen ?? new Date());
  useEffect(() => {
    if (frozen) {
      setNow(frozen);
      return;
    }
    const id = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(id);
  }, [frozen]);
  return now;
}
