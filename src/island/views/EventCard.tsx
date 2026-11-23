// A calendar block is about to start (or just has): the island opens with
// it for a moment. «Фокус» starts a session for the whole block, «Позже»
// puts it away; without a click it folds back into the compact count-down.

import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../ui/Icon";
import { isTauri } from "../../lib/ipc";
import { CATEGORY_LABEL, focusable, useAgenda } from "../../store/agenda";
import { useHub } from "../../store/hub";
import { useIsland } from "../../store/island";
import { usePrefs } from "../../store/prefs";
import { emote } from "../../store/emote";
import { durationLabel } from "../../planner/dates";

/** Minutes to the start on a ring that empties as it nears; amber once on. */
function StartRing({ minutes, lead, started }: { minutes: number; lead: number; started: boolean }) {
  const size = 46;
  const stroke = 3;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const frac = started ? 1 : Math.min(1, Math.max(0.06, minutes / Math.max(1, lead)));
  return (
    <span className={`start-ring${started ? " is-on" : ""}`} style={{ width: size, height: size }}>
      <svg className="start-ring-svg" viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <circle cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} className="start-ring-track" />
        <circle cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} className="start-ring-fill" strokeDasharray={c} strokeDashoffset={c * (1 - frac)} />
      </svg>
      {started ? <Icon name="calendar" size={17} /> : (
        <span className="start-ring-num">
          {minutes}
          <small>мин</small>
        </span>
      )}
    </span>
  );
}

export function EventCard() {
  const card = useAgenda((s) => s.card);
  const dismiss = useAgenda((s) => s.dismiss);
  const hide = useAgenda((s) => s.hide);
  const hovered = useIsland((s) => s.hovered);
  const lead = usePrefs((s) => s.islandRemind);
  const focusActive = useHub((s) => s.focus.active);
  const focusStart = useHub((s) => s.focusStart);

  useEffect(() => {
    if (card) emote(card.started ? "surprised" : "wake");
  }, [card?.event.id, card?.started]);

  // Unanswered, it folds into the compact count-down; a pointer on it holds it.
  useEffect(() => {
    if (!card || hovered) return;
    const t = window.setTimeout(hide, 20_000);
    return () => window.clearTimeout(t);
  }, [card?.event.id, card?.started, hovered, hide]);

  if (!card) return null;
  const e = card.event;
  const span = `${e.start.slice(11, 16)}–${e.end.slice(11, 16)}`;
  const meta = [span, CATEGORY_LABEL[e.category], e.location].filter(Boolean).join(" · ");
  const canFocus = focusable(e) && !focusActive;
  const open = () => {
    if (isTauri()) void invoke("planner_open", { date: e.start.slice(0, 10) });
    dismiss();
  };
  const start = () => {
    emote("determined");
    void focusStart(e.title, card.focusMin, null);
    dismiss();
  };

  return (
    <div className="view view-hello view-event" role="status">
      <div className="hello-main">
        <StartRing minutes={card.minutes} lead={lead} started={card.started} />
        <button type="button" className="hello-text event-text" onClick={open} title="Открыть в планировщике">
          <span className={`event-when${card.started ? " is-on" : ""}`}>{card.started ? "Начинается" : "Скоро в календаре"}</span>
          <span className="hello-title">{e.title}</span>
          <span className="hello-sub">{meta}</span>
        </button>
        <div className="hello-parts">
          <button type="button" className="hello-btn" onClick={dismiss}>
            Позже
          </button>
          {canFocus ? (
            <button type="button" className="hello-btn is-primary" onClick={start} title={`Фокус на ${durationLabel(card.focusMin)}`}>
              <Icon name="play" size={11} />
              Фокус {durationLabel(card.focusMin)}
            </button>
          ) : (
            <button type="button" className="hello-btn is-primary" onClick={open}>
              <Icon name="calendar" size={12} />
              Открыть
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
