import { hasActiveMedia, useSystem } from "../../store/system";
import { useRunningJobs } from "../../store/jobs";
import { Artwork } from "./MediaCard";
import { useIsland } from "../../store/island";
import { useVoice } from "../../store/voice";
import { useNextTimer, type Reminder } from "../../store/reminders";
import { clock, elapsedMs, useStopwatch, useTick } from "../../store/stopwatch";

/** Digits and a ring that drains to the end; the last ten seconds pulse. */
function TimerLeft({ t }: { t: Reminder }) {
  useTick(250);
  const due = Date.parse(t.dueAt);
  const total = Math.max(1000, due - Date.parse(t.createdAt));
  const left = Math.max(0, due - Date.now());
  const C = 2 * Math.PI * 6;
  return (
    <span className={`compact-timer${left <= 10_000 ? " is-urgent" : ""}`} title={t.text ? `Таймер «${t.text}»` : "Таймер"}>
      <svg className="timer-ring" viewBox="0 0 16 16" aria-hidden="true">
        <circle cx="8" cy="8" r="6" className="timer-ring-track" />
        <circle cx="8" cy="8" r="6" className="timer-ring-fill" strokeDasharray={C} strokeDashoffset={C * (1 - left / total)} />
      </svg>
      {clock(left + 999)}
    </span>
  );
}

/** Running digits with a blinking dot; dim while paused. */
function StopwatchTime() {
  const s = useStopwatch();
  useTick(1000, s.running);
  return (
    <span className={`compact-timer is-stopwatch${s.running ? " is-running" : ""}`} title="Секундомер">
      <span className="clock-dot" aria-hidden="true" />
      {clock(elapsedMs(s))}
    </span>
  );
}

/** The running timer, else the stopwatch, else nothing. */
export function ClockChip() {
  const timer = useNextTimer();
  const stopwatch = useStopwatch((s) => s.active);
  return timer ? <TimerLeft t={timer} /> : stopwatch ? <StopwatchTime /> : null;
}

/** Resting state: the eyes, a tiny cover when something plays, and a ring
 *  while background work (coding agents) is running. */
export function CompactView() {
  const media = useSystem((s) => s.media);
  const running = useRunningJobs();
  const withMedia = hasActiveMedia(media);
  const lip = useIsland((s) => s.topCovered);
  const voiceError=useVoice(s=>s.error);
  const timer = useNextTimer();
  const stopwatch = useStopwatch((s) => s.active);
  if (lip) return <div className="view view-compact" title={voiceError??undefined}/>;
  return (
    <div className="view view-compact">
      <div className="compact-slot compact-left">{withMedia && <Artwork size={20} radius={6} />}</div>
      <div className="compact-slot compact-right" title={voiceError??running.map((j) => j.title).join("\n")}>
        {voiceError && <span className="voice-unavailable" role="status" aria-label="Отклик на имя временно недоступен"/>}
        {timer ? <TimerLeft t={timer} /> : stopwatch ? <StopwatchTime /> : running.length > 0 && <span className="job-ring" aria-label={`Фоновых задач: ${running.length}`} />}
      </div>
    </div>
  );
}
