// Running timers and the stopwatch in the opened island: big digits, a bar
// that drains, and the buttons you reach for («+1 мин», pause, lap).

import { Icon } from "../../ui/Icon";
import { reminderDone, reminderExtend, useTimers, type Reminder } from "../../store/reminders";
import { clock, elapsedMs, stopwatchAction, useStopwatch, useTick } from "../../store/stopwatch";

/** Rows the strip takes (the island grows by 30px for each). */
export function useClockRows(): number {
  const timers = useTimers().length;
  const sw = useStopwatch((s) => s.active);
  return Math.min(2, timers) + (sw ? 1 : 0);
}

function TimerRow({ t }: { t: Reminder }) {
  useTick(250);
  const due = Date.parse(t.dueAt);
  const total = Math.max(1000, due - Date.parse(t.createdAt));
  const left = Math.max(0, due - Date.now());
  const urgent = left <= 10_000;
  return (
    <div className={`clock-row${urgent ? " is-urgent" : ""}`}>
      <Icon name="timer" size={13} />
      <span className="clock-digits">{clock(left + 999)}</span>
      <span className="clock-track" aria-hidden="true">
        <span style={{ transform: `scaleX(${left / total})` }} />
      </span>
      <span className="clock-label">{t.text || "Таймер"}</span>
      <button className="clock-btn" aria-label="Добавить минуту" onClick={() => void reminderExtend(t.id, 60)}>
        +1 мин
      </button>
      <button className="icon-btn" aria-label="Отменить таймер" onClick={() => void reminderDone(t.id)}>
        <Icon name="close" size={12} />
      </button>
    </div>
  );
}

function StopwatchRow() {
  const s = useStopwatch();
  useTick(100, s.running);
  const ms = elapsedMs(s);
  const lastMark = s.laps.at(-1);
  const lastLap = lastMark === undefined ? null : ms - lastMark;
  return (
    <div className={`clock-row is-stopwatch${s.running ? " is-running" : ""}`}>
      <span className="clock-dot" aria-hidden="true" />
      <span className="clock-digits">{clock(ms, true)}</span>
      <span className="clock-label">
        {s.laps.length ? `Круг ${s.laps.length + 1} · ${clock(lastLap ?? 0, true)}` : s.running ? "Секундомер" : "На паузе"}
      </span>
      <button className="clock-btn" onClick={() => void stopwatchAction(s.running ? "pause" : "start")}>
        {s.running ? "Пауза" : "Дальше"}
      </button>
      {s.running ? (
        <button className="clock-btn" onClick={() => void stopwatchAction("lap")}>
          Круг
        </button>
      ) : (
        <button className="clock-btn" onClick={() => void stopwatchAction("reset")}>
          Сброс
        </button>
      )}
    </div>
  );
}

export function ClocksStrip() {
  const timers = useTimers();
  const sw = useStopwatch((s) => s.active);
  if (!timers.length && !sw) return null;
  return (
    <div className="clocks-strip">
      {timers.slice(0, 2).map((t) => (
        <TimerRow key={t.id} t={t} />
      ))}
      {sw && <StopwatchRow />}
    </div>
  );
}
