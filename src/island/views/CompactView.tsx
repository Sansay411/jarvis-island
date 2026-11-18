import { hasActiveMedia, useSystem } from "../../store/system";
import { useRunningJobs } from "../../store/jobs";
import { Artwork, EqualizerBars } from "./MediaCard";
import { useIsland } from "../../store/island";
import { useVoice } from "../../store/voice";
import { useNextTimer, type Reminder } from "../../store/reminders";
import { clock, elapsedMs, useStopwatch, useTick } from "../../store/stopwatch";
import { useHub } from "../../store/hub";
import { useTts } from "../../store/tts";
import { useAgent } from "../../store/agent";
import { Icon } from "../../ui/Icon";

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

function FocusCompactLeft() {
  const focus = useHub((s) => s.focus);
  const C = 2 * Math.PI * 6;
  const frac = Math.min(1, Math.max(0, focus.remainingSec / Math.max(1, focus.totalSec)));
  return (
    <span className="compact-focus-ring" title={focus.title || "Фокус"}>
      <svg className="timer-ring is-cobalt" viewBox="0 0 16 16" aria-hidden="true">
        <circle cx="8" cy="8" r="6" className="timer-ring-track" />
        <circle cx="8" cy="8" r="6" className="timer-ring-fill is-cobalt" strokeDasharray={C} strokeDashoffset={C * (1 - frac)} />
      </svg>
    </span>
  );
}

function FocusCompactRight() {
  const focus = useHub((s) => s.focus);
  const m = Math.floor(Math.max(0, focus.remainingSec) / 60);
  const s = String(Math.max(0, focus.remainingSec) % 60).padStart(2, "0");
  return (
    <span className={`compact-timer is-focus${focus.paused ? " is-paused" : ""}`} title={`Фокус: ${focus.title}`}>
      {m}:{s}
    </span>
  );
}

/** Resting state: centered amber eyes, left media cover+equalizer / focus ring / mission badge,
 *  and right countdown / TTS live stop indicator / job spinner. */
export function CompactView() {
  const media = useSystem((s) => s.media);
  const running = useRunningJobs();
  const withMedia = hasActiveMedia(media);
  const lip = useIsland((s) => s.topCovered);
  const voiceError = useVoice((s) => s.error);
  const timer = useNextTimer();
  const stopwatch = useStopwatch((s) => s.active);
  const focus = useHub((s) => s.focus);
  const missionSteps = useHub((s) => s.missionSteps);
  const agentActive = useAgent((s) => Boolean(s.activeTurn));
  const ttsSpeaking = useTts((s) => s.speaking);
  const stopTts = useTts((s) => s.stop);

  if (lip) return <div className="view view-compact" title={voiceError ?? undefined} />;

  const doneSteps = missionSteps.filter((x) => x.status === "done").length;

  return (
    <div className="view view-compact">
      <div className="compact-side compact-left">
        {withMedia ? (
          <div className="compact-media-cluster">
            <Artwork size={20} radius={6} />
            <EqualizerBars playing={media.status === "playing"} />
          </div>
        ) : focus.active ? (
          <FocusCompactLeft />
        ) : missionSteps.length > 0 ? (
          <span className="compact-step-pill" title="План выполнения Джарвиса">
            {Math.min(missionSteps.length, doneSteps + 1)}/{missionSteps.length}
          </span>
        ) : null}
      </div>

      {/* Center 72px clear zone reserved for `Eyes.tsx` */}
      <div className="compact-center-spacer" aria-hidden="true" />

      <div className="compact-side compact-right" title={voiceError ?? running.map((j) => j.title).join("\n")}>
        {voiceError && <span className="voice-unavailable" role="status" aria-label="Отклик на имя временно недоступен" />}
        {ttsSpeaking ? (
          <button
            type="button"
            className="compact-tts-stop"
            title="Остановить озвучку Джарвиса"
            aria-label="Остановить озвучку"
            onClick={(e) => {
              e.stopPropagation();
              void stopTts();
            }}
          >
            <EqualizerBars playing accent="amber" />
            <Icon name="stop" size={9} />
          </button>
        ) : focus.active ? (
          <FocusCompactRight />
        ) : timer ? (
          <TimerLeft t={timer} />
        ) : stopwatch ? (
          <StopwatchTime />
        ) : running.length > 0 || agentActive ? (
          <span className="job-ring" aria-label="Джарвис работает" />
        ) : null}
      </div>
    </div>
  );
}
