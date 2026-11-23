// Google Calendar's small dialogs: quick create next to the slot you
// dragged, the event card with edit / delete / colour, and the full editor
// for everything else. Plus a «Начать фокус» that Google doesn't have.

import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage, isTauri } from "../lib/ipc";
import { CATEGORIES, COLORS, categoryOf, eventColor, hexOf, usePlanner, type CalEvent } from "../store/planner";
import { addDays, addMinutes, durationLabel, isoDate, isoDateTime, minutesOfDay, parseLocal, spanLabel, startOfDay, timeLabel } from "./dates";
import { PIcon } from "./icons";
import type { Draft } from "./TimeGrid";

const LAST_CAT = "jarvis.planner.lastCategory";
export const lastCategory = () => {
  try {
    return localStorage.getItem(LAST_CAT) || "personal";
  } catch {
    return "personal";
  }
};
const rememberCategory = (c: string) => {
  try {
    localStorage.setItem(LAST_CAT, c);
  } catch {
    /* session only */
  }
};

/** Place a floating card beside `anchor`, inside the window. */
function useFloating(anchor: DOMRect, gap = 12) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left = anchor.right + gap;
    if (left + w > vw - 12) left = anchor.left - w - gap;
    if (left < 12) left = Math.max(12, Math.min(vw - w - 12, anchor.left + anchor.width / 2 - w / 2));
    const top = Math.max(12, Math.min(vh - h - 12, anchor.top - 8));
    setPos({ left, top });
  }, [anchor, gap]);
  return { ref, style: pos ? { left: pos.left, top: pos.top } : { left: -9999, top: -9999 } };
}

function useEscape(onClose: () => void) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", k, true);
    return () => window.removeEventListener("keydown", k, true);
  }, [onClose]);
}

const QUARTERS = Array.from({ length: 96 }, (_, i) => i * 15);

export function TimeSelect({ value, onChange, from, label }: { value: number; onChange: (m: number) => void; from?: number; label: string }) {
  const opts = from === undefined ? QUARTERS : QUARTERS.map((q) => q + 15).filter((q) => q > from && q <= 1440);
  return (
    <select className="pl-select pl-time" aria-label={label} value={value} onChange={(e) => onChange(Number(e.target.value))}>
      {opts.map((m) => (
        <option key={m} value={m}>
          {m === 1440 ? "24:00" : timeLabel(m)}
          {from !== undefined ? ` (${durationLabel(m - from)})` : ""}
        </option>
      ))}
    </select>
  );
}

export function CategorySelect({ value, onChange }: { value: string; onChange: (c: string) => void }) {
  return (
    <span className="pl-cat-select">
      <i style={{ background: hexOf(categoryOf(value).color) }} />
      <select className="pl-select" aria-label="Календарь" value={value} onChange={(e) => onChange(e.target.value)}>
        {CATEGORIES.filter((c) => c.id !== "google" || value === "google").map((c) => (
          <option key={c.id} value={c.id}>
            {c.label}
          </option>
        ))}
      </select>
    </span>
  );
}

export function Swatches({ value, category, onChange }: { value: string; category: string; onChange: (c: string) => void }) {
  return (
    <div className="pl-swatches" role="radiogroup" aria-label="Цвет">
      <button
        type="button"
        role="radio"
        aria-checked={value === ""}
        className={`pl-swatch is-default${value === "" ? " is-on" : ""}`}
        style={{ background: hexOf(categoryOf(category).color) }}
        title="Цвет календаря"
        onClick={() => onChange("")}
      />
      {COLORS.map((c) => (
        <button
          key={c.id}
          type="button"
          role="radio"
          aria-checked={value === c.id}
          className={`pl-swatch${value === c.id ? " is-on" : ""}`}
          style={{ background: c.hex }}
          title={c.label}
          onClick={() => onChange(c.id)}
        />
      ))}
    </div>
  );
}

function Row({ icon, children }: { icon: Parameters<typeof PIcon>[0]["name"] | null; children: ReactNode }) {
  return (
    <div className="pl-row">
      <span className="pl-row-icon">{icon && <PIcon name={icon} size={20} />}</span>
      <div className="pl-row-body">{children}</div>
    </div>
  );
}

// ---------- quick create ----------

export function QuickCreate({
  draft,
  anchor,
  onDraft,
  onClose,
  onMore,
}: {
  draft: Draft;
  anchor: DOMRect;
  onDraft: (d: Draft) => void;
  onClose: () => void;
  onMore: (d: Draft, title: string, category: string) => void;
}) {
  const create = usePlanner((s) => s.create);
  const addTask = usePlanner((s) => s.addTask);
  const [title, setTitle] = useState("");
  const [kind, setKind] = useState<"event" | "task">("event");
  const [category, setCategory] = useState(lastCategory);
  const [editTime, setEditTime] = useState(false);
  const { ref, style } = useFloating(anchor);
  useEscape(onClose);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => inputRef.current?.focus(), []);

  const from = minutesOfDay(draft.start);
  const to = draft.end.getTime() - startOfDay(draft.start).getTime() >= 86_400_000 ? 1440 : minutesOfDay(draft.end);
  const setDay = (iso: string) => {
    const d = parseLocal(iso);
    if (Number.isNaN(d.getTime())) return;
    const len = draft.end.getTime() - draft.start.getTime();
    const start = draft.allDay ? d : addMinutes(d, from);
    onDraft({ ...draft, start, end: new Date(start.getTime() + len) });
  };

  const save = async () => {
    rememberCategory(category);
    if (kind === "task") {
      await addTask(title.trim() || "Задача", isoDate(draft.start), draft.allDay ? null : timeLabel(from));
    } else {
      await create({
        title: title.trim(),
        start: draft.allDay ? isoDate(draft.start) : isoDateTime(draft.start),
        end: draft.allDay ? isoDate(draft.end) : isoDateTime(draft.end),
        allDay: draft.allDay,
        category,
      });
    }
    onClose();
  };

  return (
    <div className="pl-pop-layer" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={ref} className="pl-pop pl-quick" style={style} role="dialog" aria-label="Новое мероприятие">
        <div className="pl-pop-bar">
          <button type="button" className="pl-icon-btn" aria-label="Закрыть" onClick={onClose}>
            <PIcon name="close" />
          </button>
        </div>
        <div className="pl-quick-body">
          <input
            ref={inputRef}
            className="pl-quick-title"
            placeholder={kind === "event" ? "Добавьте название и время" : "Добавьте название"}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void save()}
          />
          <div className="pl-tabs" role="tablist">
            {(["event", "task"] as const).map((k) => (
              <button key={k} type="button" role="tab" aria-selected={kind === k} className={`pl-tab${kind === k ? " is-on" : ""}`} onClick={() => setKind(k)}>
                {k === "event" ? "Мероприятие" : "Задача"}
              </button>
            ))}
          </div>
          <Row icon="clock">
            {editTime ? (
              <div className="pl-time-edit">
                <input className="pl-date-input" type="date" value={isoDate(draft.start)} onChange={(e) => setDay(e.target.value)} aria-label="Дата" />
                {!draft.allDay && (
                  <>
                    <TimeSelect
                      label="Начало"
                      value={from}
                      onChange={(m) => {
                        const len = draft.end.getTime() - draft.start.getTime();
                        const start = addMinutes(startOfDay(draft.start), m);
                        onDraft({ ...draft, start, end: new Date(start.getTime() + len) });
                      }}
                    />
                    <span className="pl-dash">–</span>
                    <TimeSelect label="Конец" value={to} from={from} onChange={(m) => onDraft({ ...draft, end: addMinutes(startOfDay(draft.start), m) })} />
                  </>
                )}
                <label className="pl-check">
                  <input
                    type="checkbox"
                    checked={draft.allDay}
                    onChange={(e) => {
                      const allDay = e.target.checked;
                      const day = startOfDay(draft.start);
                      onDraft(allDay ? { start: day, end: addDays(day, 1), allDay } : { start: addMinutes(day, 9 * 60), end: addMinutes(day, 10 * 60), allDay });
                    }}
                  />
                  <span>Весь день</span>
                </label>
              </div>
            ) : (
              <button type="button" className="pl-time-line" onClick={() => setEditTime(true)}>
                {spanLabel(
                  draft.allDay ? isoDate(draft.start) : isoDateTime(draft.start),
                  draft.allDay ? isoDate(draft.end) : isoDateTime(draft.end),
                  draft.allDay,
                )}
              </button>
            )}
          </Row>
          {kind === "event" && (
            <Row icon="calendar">
              <CategorySelect value={category} onChange={setCategory} />
            </Row>
          )}
        </div>
        <div className="pl-pop-foot">
          {kind === "event" && (
            <button type="button" className="pl-btn-text" onClick={() => onMore(draft, title, category)}>
              Другие параметры
            </button>
          )}
          <button type="button" className="pl-btn-primary" onClick={() => void save()}>
            Сохранить
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------- event card ----------

export function EventCard({ ev, anchor, onClose, onEdit }: { ev: CalEvent; anchor: DOMRect; onClose: () => void; onEdit: (e: CalEvent) => void }) {
  const update = usePlanner((s) => s.update);
  const remove = usePlanner((s) => s.remove);
  const create = usePlanner((s) => s.create);
  const showToast = usePlanner((s) => s.showToast);
  const [askSeries, setAskSeries] = useState(false);
  const [colors, setColors] = useState(false);
  const { ref, style } = useFloating(anchor);
  useEscape(onClose);
  const color = eventColor(ev);
  const start = parseLocal(ev.start);
  const end = parseLocal(ev.end);
  const now = new Date();
  const running = !ev.allDay && start <= now && end > now;
  const ahead = !ev.allDay && start > now;

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, select")) return;
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        if (ev.seriesId) setAskSeries(true);
        else {
          void remove(ev.id);
          onClose();
        }
      } else if (e.key === "e") {
        e.preventDefault();
        onEdit(ev);
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [ev, remove, onClose, onEdit]);

  const startFocus = async () => {
    const mins = Math.max(5, Math.min(240, Math.round(((running ? end.getTime() - now.getTime() : end.getTime() - start.getTime()) / 60_000))));
    try {
      if (isTauri()) await invoke("hub_focus_start", { title: ev.title, minutes: mins, taskId: null });
      showToast(`Фокус: «${ev.title}», ${durationLabel(mins)}`);
      onClose();
    } catch (e) {
      showToast(errorMessage(e));
    }
  };

  return (
    <div className="pl-pop-layer" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={ref} className="pl-pop pl-card" style={style} role="dialog" aria-label={ev.title}>
        <div className="pl-pop-bar">
          <button type="button" className="pl-icon-btn" aria-label="Изменить" title="Изменить (E)" onClick={() => onEdit(ev)}>
            <PIcon name="edit" />
          </button>
          <button
            type="button"
            className="pl-icon-btn"
            aria-label="Удалить"
            title="Удалить (Delete)"
            onClick={() => {
              if (ev.seriesId) setAskSeries(true);
              else {
                void remove(ev.id);
                onClose();
              }
            }}
          >
            <PIcon name="trash" />
          </button>
          <button type="button" className="pl-icon-btn" aria-label="Цвет" title="Цвет" onClick={() => setColors((v) => !v)}>
            <span className="pl-dot-btn" style={{ background: hexOf(color) }} />
          </button>
          <button
            type="button"
            className="pl-icon-btn"
            aria-label="Копировать"
            title="Создать копию"
            onClick={() => {
              void create({ title: ev.title, notes: ev.notes, location: ev.location, start: ev.start, end: ev.end, allDay: ev.allDay, color: ev.color, category: ev.category === "google" ? "personal" : ev.category });
              onClose();
            }}
          >
            <PIcon name="copy" />
          </button>
          <span className="pl-pop-gap" />
          <button type="button" className="pl-icon-btn" aria-label="Закрыть" onClick={onClose}>
            <PIcon name="close" />
          </button>
        </div>
        {colors && (
          <div className="pl-card-colors">
            <Swatches value={ev.color} category={ev.category} onChange={(c) => void update(ev.id, { color: c })} />
          </div>
        )}
        {askSeries && (
          <div className="pl-card-ask">
            <span>Удалить повторяющееся мероприятие</span>
            <div>
              <button type="button" className="pl-btn-text" onClick={() => { void remove(ev.id, false); onClose(); }}>
                Только это
              </button>
              <button type="button" className="pl-btn-text" onClick={() => { void remove(ev.id, true); onClose(); }}>
                Это и следующие
              </button>
            </div>
          </div>
        )}
        <div className="pl-card-body">
          <div className="pl-card-head">
            <span className="pl-card-square" style={{ background: hexOf(color) }} />
            <div>
              <div className={`pl-card-title${ev.done ? " is-done" : ""}`}>{ev.title}</div>
              <div className="pl-card-when">{spanLabel(ev.start, ev.end, ev.allDay)}</div>
              {(ev.seriesId || ev.recurring) && <div className="pl-card-when">Повторяется</div>}
            </div>
          </div>
          {ev.location && (
            <Row icon="place">
              <span>{ev.location}</span>
            </Row>
          )}
          {ev.notes && (
            <Row icon="notes">
              <span className="pl-card-notes">{ev.notes}</span>
            </Row>
          )}
          <Row icon="calendar">
            <span>{categoryOf(ev.category).label}</span>
            {ev.googleId && <span className="pl-card-meta"> · есть в Google</span>}
          </Row>
        </div>
        <div className="pl-card-actions">
          {(running || ahead) && (
            <button type="button" className="pl-chip-btn is-accent" onClick={() => void startFocus()}>
              <PIcon name="focus" size={18} />
              {running ? "Фокус до конца блока" : "Начать фокус"}
            </button>
          )}
          <button type="button" className={`pl-chip-btn${ev.done ? " is-on" : ""}`} onClick={() => void update(ev.id, { done: !ev.done })}>
            <PIcon name={ev.done ? "task" : "circle"} size={18} />
            {ev.done ? "Выполнено" : "Отметить выполненным"}
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------- full editor ----------

export interface EditorInit {
  ev?: CalEvent;
  draft?: Draft;
  title?: string;
  category?: string;
}

export function EventEditor({ init, onClose }: { init: EditorInit; onClose: () => void }) {
  const update = usePlanner((s) => s.update);
  const create = usePlanner((s) => s.create);
  const remove = usePlanner((s) => s.remove);
  const ev = init.ev;
  const s0 = ev ? parseLocal(ev.start) : init.draft!.start;
  const e0 = ev ? parseLocal(ev.end) : init.draft!.end;
  const [title, setTitle] = useState(ev?.title ?? init.title ?? "");
  const [allDay, setAllDay] = useState(ev?.allDay ?? init.draft?.allDay ?? false);
  const [startDay, setStartDay] = useState(isoDate(s0));
  const [endDay, setEndDay] = useState(isoDate(allDay ? addDays(e0, -1) : e0));
  const [from, setFrom] = useState(allDay ? 9 * 60 : minutesOfDay(s0));
  const [to, setTo] = useState(allDay ? 10 * 60 : minutesOfDay(e0) === 0 && isoDate(e0) !== isoDate(s0) ? 1440 : minutesOfDay(e0));
  const [location, setLocation] = useState(ev?.location ?? "");
  const [notes, setNotes] = useState(ev?.notes ?? "");
  const [category, setCategory] = useState(ev?.category ?? init.category ?? lastCategory());
  const [color, setColor] = useState(ev?.color ?? "");
  const [repeat, setRepeat] = useState<"none" | "daily" | "weekdays" | "weekly">("none");
  useEscape(onClose);
  const titleRef = useRef<HTMLInputElement>(null);
  useEffect(() => titleRef.current?.focus(), []);

  const span = () => {
    if (allDay) {
      const a = parseLocal(startDay);
      const b = parseLocal(endDay);
      return { start: isoDate(a), end: isoDate(addDays(b < a ? a : b, 1)) };
    }
    const a = addMinutes(parseLocal(startDay), from);
    let b = addMinutes(parseLocal(endDay), to === 1440 ? 1440 : to);
    if (b <= a) b = addMinutes(a, 60);
    return { start: isoDateTime(a), end: isoDateTime(b) };
  };

  const save = async () => {
    const { start, end } = span();
    rememberCategory(category);
    if (ev) await update(ev.id, { title: title.trim() || ev.title, start, end, allDay, location, notes, category, color });
    else await create({ title: title.trim(), start, end, allDay, location, notes, category, color, repeat });
    onClose();
  };

  return (
    <div className="pl-modal-layer" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="pl-editor" role="dialog" aria-label={ev ? "Изменить мероприятие" : "Новое мероприятие"}>
        <div className="pl-editor-bar">
          <button type="button" className="pl-icon-btn" aria-label="Закрыть" onClick={onClose}>
            <PIcon name="close" />
          </button>
          <input
            ref={titleRef}
            className="pl-editor-title"
            placeholder="Добавьте название"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && (e.ctrlKey || e.metaKey) && void save()}
          />
          <button type="button" className="pl-btn-primary" onClick={() => void save()}>
            Сохранить
          </button>
        </div>
        <div className="pl-editor-body">
          <Row icon="clock">
            <div className="pl-time-edit">
              <input className="pl-date-input" type="date" value={startDay} onChange={(e) => {
                const v = e.target.value;
                // Keep the length when the start moves.
                const shift = Math.round((parseLocal(v).getTime() - parseLocal(startDay).getTime()) / 86_400_000);
                setStartDay(v);
                if (!Number.isNaN(shift)) setEndDay(isoDate(addDays(parseLocal(endDay), shift)));
              }} aria-label="Дата начала" />
              {!allDay && <TimeSelect label="Начало" value={from} onChange={(m) => { setTo(Math.min(1440, m + (to - from))); setFrom(m); }} />}
              <span className="pl-dash">–</span>
              {!allDay && <TimeSelect label="Конец" value={to} from={startDay === endDay ? from : undefined} onChange={setTo} />}
              <input className="pl-date-input" type="date" value={endDay} min={startDay} onChange={(e) => setEndDay(e.target.value)} aria-label="Дата окончания" />
            </div>
            <div className="pl-time-edit">
              <label className="pl-check">
                <input type="checkbox" checked={allDay} onChange={(e) => setAllDay(e.target.checked)} />
                <span>Весь день</span>
              </label>
              {!ev && (
                <select className="pl-select" aria-label="Повтор" value={repeat} onChange={(e) => setRepeat(e.target.value as typeof repeat)}>
                  <option value="none">Не повторяется</option>
                  <option value="daily">Каждый день (30 дней)</option>
                  <option value="weekdays">По будням (6 недель)</option>
                  <option value="weekly">Каждую неделю (12 недель)</option>
                </select>
              )}
            </div>
          </Row>
          <Row icon="place">
            <input className="pl-field" placeholder="Добавить место" value={location} onChange={(e) => setLocation(e.target.value)} />
          </Row>
          <Row icon="notes">
            <textarea className="pl-field pl-notes" placeholder="Добавить описание" rows={4} value={notes} onChange={(e) => setNotes(e.target.value)} />
          </Row>
          <Row icon="calendar">
            <CategorySelect value={category} onChange={setCategory} />
          </Row>
          <Row icon={null}>
            <Swatches value={color} category={category} onChange={setColor} />
          </Row>
        </div>
        {ev && (
          <div className="pl-editor-foot">
            <button
              type="button"
              className="pl-btn-text is-danger"
              onClick={() => {
                void remove(ev.id);
                onClose();
              }}
            >
              Удалить
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
