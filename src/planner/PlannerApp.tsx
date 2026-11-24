// The planner window: Google Calendar's layout — header with «Сегодня»,
// arrows and the period, a sidebar with «Создать», the small month and
// «Мои календари», and the grid. JARVIS adds «Спланировать день»: free time
// filled with blocks for work, learning and sport, shown dashed until
// accepted. Keyboard: T today, D / X / W / M views, J / K or N / P to page,
// C to create, Esc to close.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../lib/ipc";
import { CATEGORIES, hexOf, startPlannerFeed, usePlanner, type CalEvent } from "../store/planner";
import { addDays, addMinutes, isoDate, minutesOfDay, periodTitle, sameDay, startOfDay, viewRange, type ViewKind } from "./dates";
import { PIcon } from "./icons";
import { MiniMonth } from "./MiniMonth";
import { MonthView } from "./MonthView";
import { EventCard, EventEditor, QuickCreate, lastCategory, type EditorInit } from "./Popovers";
import { TimeGrid, useNow, type Draft } from "./TimeGrid";
import { MentorPanel } from "./MentorPanel";
import "./planner.css";

const VIEWS: Array<{ id: ViewKind; label: string; key: string }> = [
  { id: "day", label: "День", key: "D" },
  { id: "week", label: "Неделя", key: "W" },
  { id: "month", label: "Месяц", key: "M" },
  { id: "4day", label: "4 дня", key: "X" },
];

type Pop = { kind: "create"; draft: Draft; anchor: DOMRect } | { kind: "event"; id: string; anchor: DOMRect } | null;

function ago(iso: string | null, now: Date): string {
  if (!iso) return "ещё не синхронизирован";
  const min = Math.max(0, Math.round((now.getTime() - new Date(iso).getTime()) / 60_000));
  if (min < 1) return "синхронизирован только что";
  if (min < 60) return `синхронизирован ${min} мин назад`;
  return `синхронизирован в ${new Date(iso).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" })}`;
}

export default function PlannerApp() {
  useEffect(() => {
    void startPlannerFeed();
  }, []);
  const view = usePlanner((s) => s.view);
  const anchor = usePlanner((s) => s.anchor);
  const events = usePlanner((s) => s.events);
  const tasks = usePlanner((s) => s.tasks);
  const hidden = usePlanner((s) => s.hidden);
  const suggestions = usePlanner((s) => s.suggestions);
  const suggesting = usePlanner((s) => s.suggesting);
  const toast = usePlanner((s) => s.toast);
  const sync = usePlanner((s) => s.sync);
  const frozen = usePlanner((s) => s.now);
  const st = usePlanner.getState;
  const now = useNow(frozen);

  const [sidebar, setSidebar] = useState(true);
  const [mentor, setMentorState] = useState(() => {
    try {
      return localStorage.getItem("jarvis.planner.mentor") !== "0";
    } catch {
      return true;
    }
  });
  const setMentor = (fn: (v: boolean) => boolean) =>
    setMentorState((v) => {
      const next = fn(v);
      try {
        localStorage.setItem("jarvis.planner.mentor", next ? "1" : "0");
      } catch {
        /* session only */
      }
      return next;
    });
  const [viewMenu, setViewMenu] = useState(false);
  const [planAsk, setPlanAsk] = useState(false);
  const [wishes, setWishes] = useState("");
  const [pop, setPop] = useState<Pop>(null);
  const [editor, setEditor] = useState<EditorInit | null>(null);
  const [calsOpen, setCalsOpen] = useState(true);
  const createRef = useRef<HTMLButtonElement>(null);

  const visible = useMemo(() => events.filter((e) => !hidden.includes(e.category)), [events, hidden]);
  const { start, days } = viewRange(view, anchor);
  const dayList = useMemo(() => (view === "month" ? [] : Array.from({ length: days }, (_, i) => addDays(start, i))), [view, start.getTime(), days]); // eslint-disable-line react-hooks/exhaustive-deps
  const selectedId = pop?.kind === "event" ? pop.id : null;
  const selected = selectedId ? events.find((e) => e.id === selectedId) ?? null : null;

  const quickCreateAt = useCallback(
    (day: Date, anchorRect: DOMRect) => {
      // The next whole hour today, nine in the morning on other days.
      const base = startOfDay(day);
      const minutes = sameDay(day, now) ? Math.min(23 * 60, (Math.floor(minutesOfDay(now) / 60) + 1) * 60) : 9 * 60;
      const s = addMinutes(base, minutes);
      setPop({ kind: "create", draft: { start: s, end: addMinutes(s, 60), allDay: false }, anchor: anchorRect });
    },
    [now],
  );

  // Google Calendar's keyboard shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, select, [contenteditable]") || e.ctrlKey || e.metaKey || e.altKey) return;
      if (pop || editor) return;
      const k = e.key.toLowerCase();
      const map: Record<string, () => void> = {
        t: () => st().goToday(),
        "е": () => st().goToday(),
        d: () => st().setView("day"),
        w: () => st().setView("week"),
        m: () => st().setView("month"),
        x: () => st().setView("4day"),
        j: () => st().stepBy(1),
        n: () => st().stepBy(1),
        k: () => st().stepBy(-1),
        p: () => st().stepBy(-1),
        arrowright: () => st().stepBy(1),
        arrowleft: () => st().stepBy(-1),
        c: () =>
          quickCreateAt(
            dayList.some((d) => sameDay(d, now)) || view === "month" ? now : anchor,
            createRef.current?.getBoundingClientRect() ?? new DOMRect(window.innerWidth / 2 - 200, 120, 0, 0),
          ),
      };
      const fn = map[k];
      if (fn) {
        e.preventDefault();
        fn();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pop, editor, view, anchor, now, dayList, quickCreateAt, st]);

  const onMove = (ev: CalEvent, s: Date, e: Date, allDay: boolean) => {
    const fmt = (d: Date) => (allDay ? isoDate(d) : `${isoDate(d)}T${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`);
    void st().update(ev.id, { start: fmt(s), end: fmt(e), allDay }, { undoLabel: "Мероприятие перенесено" });
  };

  const planDate = view === "month" ? (sameDay(startOfDay(anchor), startOfDay(now)) ? now : anchor) : dayList.find((d) => sameDay(d, now)) ?? dayList[0] ?? anchor;

  const win = (fn: (w: ReturnType<typeof getCurrentWindow>) => Promise<void>) => {
    if (isTauri()) void fn(getCurrentWindow());
  };

  return (
    <div className={`planner${sidebar ? "" : " no-sidebar"}${mentor ? " with-mentor" : ""}`}>
      <header className="pl-header" data-tauri-drag-region>
        <div className="pl-header-left">
          <button type="button" className="pl-icon-btn" aria-label="Главное меню" title="Главное меню" onClick={() => setSidebar((v) => !v)}>
            <PIcon name="menu" />
          </button>
          <div className="pl-brand" data-tauri-drag-region>
            <span className="pl-brand-mark" aria-hidden="true">
              <i />
              <i />
            </span>
            <span className="pl-brand-name" data-tauri-drag-region>
              Календарь
            </span>
          </div>
        </div>
        <button type="button" className="pl-today" onClick={() => st().goToday()} title={now.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" })}>
          Сегодня
        </button>
        <div className="pl-arrows">
          <button type="button" className="pl-icon-btn" aria-label="Назад" title="Назад (K)" onClick={() => st().stepBy(-1)}>
            <PIcon name="left" />
          </button>
          <button type="button" className="pl-icon-btn" aria-label="Вперёд" title="Вперёд (J)" onClick={() => st().stepBy(1)}>
            <PIcon name="right" />
          </button>
        </div>
        <h1 className="pl-period" data-tauri-drag-region>
          {periodTitle(view, anchor)}
        </h1>
        <div className="pl-header-gap" data-tauri-drag-region />
        <div className="pl-plan-wrap">
          <button
            type="button"
            className={`pl-plan-btn${suggesting ? " is-busy" : ""}`}
            onClick={() => setPlanAsk((v) => !v)}
            disabled={suggesting}
            aria-expanded={planAsk}
            title="Джарвис спланирует свободное время дня под ваши дела и цели"
          >
            <PIcon name="spark" size={18} />
            {suggesting ? "Планирую…" : "Спланировать день"}
          </button>
          {planAsk && (
            <>
              <div className="pl-menu-layer" onPointerDown={() => setPlanAsk(false)} />
              <form
                className="pl-menu pl-plan-ask"
                onSubmit={(e) => {
                  e.preventDefault();
                  setPlanAsk(false);
                  void st().suggest(planDate, wishes);
                }}
              >
                <label htmlFor="pl-wishes">Что важно в этот день?</label>
                <textarea
                  id="pl-wishes"
                  autoFocus
                  rows={3}
                  value={wishes}
                  placeholder="Например: закончить отчёт до обеда, вечером зал, в 19:00 ужин с друзьями"
                  onChange={(e) => setWishes(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      e.currentTarget.form?.requestSubmit();
                    }
                    if (e.key === "Escape") setPlanAsk(false);
                  }}
                />
                <div className="pl-plan-ask-row">
                  <span>Можно оставить пустым: Джарвис возьмёт задачи и цели.</span>
                  <button type="submit" className="pl-plan-go">
                    Спланировать
                  </button>
                </div>
              </form>
            </>
          )}
        </div>
        {sync?.connected && (
          <button type="button" className={`pl-icon-btn${sync.syncing ? " is-spinning" : ""}`} aria-label="Синхронизировать" title={`Google: ${ago(sync.lastSync, now)}`} onClick={() => void st().syncNow()}>
            <PIcon name="sync" />
          </button>
        )}
        <div className="pl-view-switch">
          <button type="button" className="pl-view-btn" aria-haspopup="menu" aria-expanded={viewMenu} onClick={() => setViewMenu((v) => !v)}>
            {VIEWS.find((v) => v.id === view)?.label}
            <PIcon name="down" size={18} />
          </button>
          {viewMenu && (
            <>
              <div className="pl-menu-layer" onPointerDown={() => setViewMenu(false)} />
              <div className="pl-menu" role="menu">
                {VIEWS.map((v) => (
                  <button
                    key={v.id}
                    type="button"
                    role="menuitemradio"
                    aria-checked={view === v.id}
                    className={`pl-menu-item${view === v.id ? " is-on" : ""}`}
                    onClick={() => {
                      st().setView(v.id);
                      setViewMenu(false);
                    }}
                  >
                    <span>{v.label}</span>
                    <kbd>{v.key}</kbd>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
        <button type="button" className={`pl-icon-btn pl-mentor-toggle${mentor ? " is-on" : ""}`} aria-label="Наставник" title="Наставник Джарвиса" onClick={() => setMentor((v) => !v)}>
          <span className="pl-eyes" aria-hidden="true">
            <i />
            <i />
          </span>
        </button>
        <div className="pl-win">
          <button type="button" className="pl-win-btn" aria-label="Свернуть" onClick={() => win((w) => w.minimize())}>
            <PIcon name="minimize" size={16} />
          </button>
          <button type="button" className="pl-win-btn" aria-label="Развернуть" onClick={() => win((w) => w.toggleMaximize())}>
            <PIcon name="maximize" size={14} />
          </button>
          <button type="button" className="pl-win-btn is-close" aria-label="Закрыть" onClick={() => win((w) => w.close())}>
            <PIcon name="close" size={16} />
          </button>
        </div>
      </header>

      <div className="pl-main">
        {sidebar && (
          <aside className="pl-side">
            <button
              ref={createRef}
              type="button"
              className="pl-create"
              onClick={(e) => quickCreateAt(dayList.some((d) => sameDay(d, now)) || view === "month" ? now : anchor, e.currentTarget.getBoundingClientRect())}
            >
              <PIcon name="plus" size={24} />
              <span>Создать</span>
            </button>
            <MiniMonth anchor={anchor} now={now} rangeStart={start} rangeDays={days} onPick={(d) => st().setAnchor(d)} />
            <div className="pl-cals">
              <button type="button" className="pl-cals-head" onClick={() => setCalsOpen((v) => !v)} aria-expanded={calsOpen}>
                <span>Мои календари</span>
                <PIcon name={calsOpen ? "down" : "right"} size={18} />
              </button>
              {calsOpen &&
                CATEGORIES.filter((c) => c.id !== "google" || sync?.connected || events.some((e) => e.category === "google")).map((c) => {
                  const on = !hidden.includes(c.id);
                  return (
                    <label key={c.id} className="pl-cal">
                      <input type="checkbox" checked={on} onChange={() => st().toggleHidden(c.id)} />
                      <span className="pl-cal-box" style={{ ["--c" as string]: hexOf(c.color) }} aria-hidden="true">
                        {on && (
                          <svg viewBox="0 0 12 12">
                            <path d="m2.4 6.2 2.4 2.3 4.8-5" />
                          </svg>
                        )}
                      </span>
                      <span className="pl-cal-name">{c.label}</span>
                    </label>
                  );
                })}
            </div>
            <div className="pl-side-gap" />
            <div className="pl-google">
              {sync?.connected ? (
                <>
                  <PIcon name="google" size={18} />
                  <div className="pl-google-text">
                    <b>{sync.account || "Google Календарь"}</b>
                    <span className={sync.error ? "is-error" : ""}>{sync.error ?? (sync.syncing ? "синхронизация…" : ago(sync.lastSync, now))}</span>
                  </div>
                </>
              ) : (
                <button type="button" className="pl-google-connect" onClick={() => isTauri() && void invoke("settings_open")}>
                  <PIcon name="google" size={18} />
                  <span>Подключить Google Календарь</span>
                </button>
              )}
            </div>
          </aside>
        )}

        <section className="pl-surface">
          {(suggestions.length > 0 || suggesting) && (
            <div className="pl-suggest-bar">
              <PIcon name="spark" size={18} />
              <span>
                {suggesting
                  ? "Джарвис ищет свободное время и составляет план…"
                  : `План на ${planDate.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" })}: ${suggestions.length} ${suggestions.length === 1 ? "блок" : suggestions.length < 5 ? "блока" : "блоков"}. Нажмите на блок, чтобы добавить его.`}
              </span>
              {!suggesting && (
                <>
                  <button type="button" className="pl-btn-text" onClick={() => st().dismissSuggestions()}>
                    Скрыть
                  </button>
                  <button type="button" className="pl-btn-primary is-sm" onClick={() => void st().acceptAll()}>
                    Добавить все
                  </button>
                </>
              )}
            </div>
          )}
          {view === "month" ? (
            <MonthView
              start={start}
              month={anchor.getMonth()}
              events={visible}
              tasks={tasks}
              now={now}
              selectedId={selectedId}
              onCreate={(draft, a) => setPop({ kind: "create", draft, anchor: a })}
              onOpen={(e, a) => setPop({ kind: "event", id: e.id, anchor: a })}
              onMove={onMove}
              onDay={(d) => {
                st().setAnchor(d);
                st().setView("day");
              }}
              onTask={(t) => void st().toggleTask(t.id)}
            />
          ) : (
            <TimeGrid
              key={view}
              days={dayList}
              events={visible}
              tasks={tasks}
              suggestions={suggestions}
              now={now}
              scrollToHour={8}
              selectedId={selectedId}
              draft={pop?.kind === "create" ? pop.draft : null}
              onCreate={(draft, a) => setPop({ kind: "create", draft, anchor: a })}
              onOpen={(e, a) => setPop({ kind: "event", id: e.id, anchor: a })}
              onEdit={(e) => {
                setPop(null);
                setEditor({ ev: e });
              }}
              onMove={onMove}
              onDay={(d) => {
                st().setAnchor(d);
                st().setView("day");
              }}
              onTask={(t) => void st().toggleTask(t.id)}
              onAccept={(s) => void st().accept(s)}
            />
          )}
        </section>

        {mentor && <MentorPanel now={now} />}
      </div>

      {pop?.kind === "create" && (
        <QuickCreate
          draft={pop.draft}
          anchor={pop.anchor}
          onDraft={(draft) => setPop({ ...pop, draft })}
          onClose={() => setPop(null)}
          onMore={(draft, title, category) => {
            setPop(null);
            setEditor({ draft, title, category: category || lastCategory() });
          }}
        />
      )}
      {pop?.kind === "event" && selected && (
        <EventCard
          ev={selected}
          anchor={pop.anchor}
          onClose={() => setPop(null)}
          onEdit={(e) => {
            setPop(null);
            setEditor({ ev: e });
          }}
        />
      )}
      {editor && <EventEditor init={editor} onClose={() => setEditor(null)} />}

      {toast && (
        <div className="pl-toast" role="status" key={toast.id}>
          <span>{toast.text}</span>
          {toast.undo && (
            <button
              type="button"
              onClick={() => {
                const u = toast.undo!;
                usePlanner.setState({ toast: null });
                void u.run();
              }}
            >
              {toast.undo.label}
            </button>
          )}
          <button type="button" className="pl-toast-x" aria-label="Закрыть" onClick={() => usePlanner.setState({ toast: null })}>
            <PIcon name="close" size={16} />
          </button>
        </div>
      )}
    </div>
  );
}
