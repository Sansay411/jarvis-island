// Month view: six weeks of cells. Multi-day and all-day events run as bars
// across a week, timed ones are «● 10:00 Название» lines; what does not fit
// folds into «Ещё N». Click a cell to add an all-day event, drag an event
// to another day to move it.

import { useLayoutEffect, useMemo, useRef, useState, type PointerEvent as RPointerEvent } from "react";
import type { TaskItem } from "../lib/types";
import { eventColor, hexOf, inkOn, type CalEvent } from "../store/planner";
import { addDays, hm, isoDate, MONTHS_SHORT, parseLocal, sameDay, startOfDay, WEEKDAYS_SHORT } from "./dates";
import type { Draft } from "./TimeGrid";
import { isBanner, layoutBanners } from "./layout";

const ROW_H = 22;

interface Props {
  start: Date;
  month: number;
  events: CalEvent[];
  tasks: TaskItem[];
  now: Date;
  selectedId: string | null;
  onCreate: (d: Draft, anchor: DOMRect) => void;
  onOpen: (e: CalEvent, anchor: DOMRect) => void;
  onMove: (e: CalEvent, start: Date, end: Date, allDay: boolean) => void;
  onDay: (d: Date) => void;
  onTask: (t: TaskItem) => void;
}

export function MonthView(p: Props) {
  const gridRef = useRef<HTMLDivElement>(null);
  const [cellH, setCellH] = useState(110);
  useLayoutEffect(() => {
    const el = gridRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setCellH(el.clientHeight / 6));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const capacity = Math.max(1, Math.floor((cellH - 30) / ROW_H));

  const weeks = useMemo(() => Array.from({ length: 6 }, (_, w) => addDays(p.start, w * 7)), [p.start]);
  const [drag, setDrag] = useState<{ ev: CalEvent; from: number; over: number; moved: boolean; x: number; y: number; el: HTMLElement } | null>(null);

  const cellIndexAt = (x: number, y: number) => {
    const r = gridRef.current!.getBoundingClientRect();
    const col = Math.max(0, Math.min(6, Math.floor(((x - r.left) / r.width) * 7)));
    const row = Math.max(0, Math.min(5, Math.floor(((y - r.top) / r.height) * 6)));
    return row * 7 + col;
  };

  const down = (e: RPointerEvent<HTMLElement>, ev: CalEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    gridRef.current!.setPointerCapture(e.pointerId);
    const at = cellIndexAt(e.clientX, e.clientY);
    setDrag({ ev, from: at, over: at, moved: false, x: e.clientX, y: e.clientY, el: e.currentTarget as HTMLElement });
  };

  return (
    <div className="pl-month">
      <div className="pl-month-heads">
        {WEEKDAYS_SHORT.map((w) => (
          <div key={w} className="pl-month-dow">
            {w}
          </div>
        ))}
      </div>
      <div
        ref={gridRef}
        className="pl-month-grid"
        onPointerMove={(e) => {
          if (!drag) return;
          const over = cellIndexAt(e.clientX, e.clientY);
          setDrag({ ...drag, over, moved: drag.moved || Math.abs(e.clientX - drag.x) > 4 || Math.abs(e.clientY - drag.y) > 4 });
        }}
        onPointerUp={() => {
          const d = drag;
          setDrag(null);
          if (!d) return;
          if (!d.moved) {
            p.onOpen(d.ev, d.el.getBoundingClientRect());
            return;
          }
          const shift = d.over - d.from;
          if (shift === 0) return;
          p.onMove(d.ev, addDays(parseLocal(d.ev.start), shift), addDays(parseLocal(d.ev.end), shift), d.ev.allDay);
        }}
      >
        {weeks.map((ws, w) => {
          const bars = layoutBanners(p.events, ws, 7);
          const cells = Array.from({ length: 7 }, (_, i) => {
            const d = addDays(ws, i);
            const onCell = bars.filter((b) => b.from <= i && b.to >= i);
            const taken = onCell.reduce((m, b) => Math.max(m, b.row + 1), 0);
            const items = p.events.filter((e) => !isBanner(e) && sameDay(parseLocal(e.start), d)).sort((a, b) => a.start.localeCompare(b.start));
            const tasks = p.tasks.filter((t) => t.scheduledDate === isoDate(d));
            return { d, onCell, taken, items, tasks, total: taken + items.length + tasks.length };
          });
          // One row stays free for «Ещё N» when anything in the week overflows.
          const overflow = cells.some((c) => c.total > capacity);
          const barLimit = overflow ? capacity - 1 : capacity;
          return (
            <div key={isoDate(ws)} className="pl-month-week">
              {cells.map((c, i) => {
                const { d } = c;
                const idx = w * 7 + i;
                const today = sameDay(d, p.now);
                const outside = d.getMonth() !== p.month;
                const rows = Math.min(c.taken, barLimit);
                const room = Math.max(0, (c.total > capacity ? capacity - 1 : capacity) - rows);
                const shown = c.items.slice(0, room);
                const shownTasks = c.tasks.slice(0, Math.max(0, room - shown.length));
                const hiddenCount =
                  c.onCell.filter((b) => b.row >= barLimit).length + (c.items.length - shown.length) + (c.tasks.length - shownTasks.length);
                const isOver = drag?.moved && drag.over === idx;
                return (
                  <div
                    key={i}
                    className={`pl-month-cell${outside ? " is-outside" : ""}${isOver ? " is-over" : ""}`}
                    onClick={(e) => {
                      if ((e.target as HTMLElement).closest(".pl-mchip, .pl-mdate, .pl-mmore, .pl-task, .pl-mbar")) return;
                      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                      p.onCreate({ start: startOfDay(d), end: addDays(startOfDay(d), 1), allDay: true }, r);
                    }}
                  >
                    <button type="button" className={`pl-mdate${today ? " is-today" : ""}`} onClick={() => p.onDay(d)}>
                      {d.getDate() === 1 ? `${d.getDate()} ${MONTHS_SHORT[d.getMonth()]}` : d.getDate()}
                    </button>
                    <div className="pl-month-items" style={{ paddingTop: rows * ROW_H }}>
                      {shown.map((ev) => {
                        const color = eventColor(ev);
                        return (
                          <div
                            key={ev.id}
                            className={`pl-mchip${p.selectedId === ev.id ? " is-selected" : ""}${parseLocal(ev.end) < p.now ? " is-past" : ""}${ev.done ? " is-done" : ""}`}
                            onPointerDown={(e) => down(e, ev)}
                            title={ev.title}
                          >
                            <i style={{ background: hexOf(color) }} />
                            <span className="pl-mchip-time">{hm(parseLocal(ev.start))}</span>
                            <span className="pl-mchip-title">{ev.title}</span>
                          </div>
                        );
                      })}
                      {shownTasks.map((t) => (
                        <button key={t.id} type="button" className={`pl-task pl-task-month${t.completed ? " is-done" : ""}`} onClick={() => p.onTask(t)}>
                          <svg className="pl-task-mark" viewBox="0 0 16 16" aria-hidden="true">
                            <circle cx="8" cy="8" r="6.2" />
                            {t.completed && <path d="m5.2 8.2 1.9 1.9 3.8-4.1" />}
                          </svg>
                          <span>{t.title}</span>
                        </button>
                      ))}
                      {hiddenCount > 0 && (
                        <button type="button" className="pl-mmore" onClick={() => p.onDay(d)}>
                          Ещё {hiddenCount}
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
              {bars
                .filter((b) => b.row < barLimit)
                .map((b) => {
                  const color = eventColor(b.item);
                  return (
                    <div
                      key={b.item.id}
                      className={`pl-mbar${p.selectedId === b.item.id ? " is-selected" : ""}${b.clippedStart ? " cut-start" : ""}${b.clippedEnd ? " cut-end" : ""}`}
                      style={{
                        left: `calc(${(b.from / 7) * 100}% + 4px)`,
                        width: `calc(${((b.to - b.from + 1) / 7) * 100}% - 8px)`,
                        top: 28 + b.row * ROW_H,
                        background: hexOf(color),
                        color: inkOn(color),
                      }}
                      onPointerDown={(e) => down(e, b.item)}
                      title={b.item.title}
                    >
                      {b.item.allDay ? b.item.title : `${hm(parseLocal(b.item.start))} ${b.item.title}`}
                    </div>
                  );
                })}
            </div>
          );
        })}
      </div>
    </div>
  );
}
