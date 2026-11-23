import { useEffect, useState } from "react";
import { hasActiveMedia, useSystem } from "../../store/system";
import { useRunningJobs } from "../../store/jobs";
import { Artwork, EqualizerBars } from "./MediaCard";
import { useIsland } from "../../store/island";
import { useVoice } from "../../store/voice";
import { useNextTimer, type Reminder } from "../../store/reminders";
import { clock, elapsedMs, useStopwatch, useTick } from "../../store/stopwatch";
import { useHub } from "../../store/hub";
import { useMentor, type Watching } from "../../store/mentor";
import { useTts } from "../../store/tts";
import { useAgent } from "../../store/agent";
import { Icon } from "../../ui/Icon";
import { isCharging, levelOf, levelTone, primaryAudio, useBluetooth } from "../../store/bluetooth";
import { useAgenda, type Upcoming } from "../../store/agenda";
import { kindIcon } from "./SoundDeck";

/** Headphones running low, at rest: the one thing worth a glance. */
function LowHeadset() {
  const main = useBluetooth((s) => primaryAudio(s.devices));
  if (!main || isCharging(main)) return null;
  const level = levelOf(main);
  if (level == null || level > 20) return null;
  return (
    <span className={`compact-battery is-${levelTone(level)}`} title={`${main.model ?? main.name}: ${level}%`}>
      <Icon name={kindIcon(main.kind)} size={11} />
      {level}%
    </span>
  );
}

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
      <svg className="timer-ring" viewBox="0 0 16 16" aria-hidden="true">
        <circle cx="8" cy="8" r="6" className="timer-ring-track" />
        <circle cx="8" cy="8" r="6" className="timer-ring-fill" strokeDasharray={C} strokeDashoffset={C * (1 - frac)} />
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

const PHASE_LABEL = { thinking: "Думаю", acting: "Делаю", verifying: "Проверяю" } as const;

/** JARVIS works on a request: what it is doing, in a few words, and for how long. */
function ThinkingCompact() {
  const phase = useAgent((s) => s.phase);
  // Primitive selectors: a fresh object per read would re-render forever.
  const toolTitle = useAgent((s) => {
    const it = s.items.findLast((i) => i.kind === "tool" && (i.status === "running" || i.status === "awaiting"));
    return it?.kind === "tool" ? it.title : null;
  });
  const awaiting = useAgent((s) => s.items.some((i) => i.kind === "tool" && i.status === "awaiting"));
  const turn = useAgent((s) => s.activeTurn);
  const [since, setSince] = useState(() => Date.now());
  useEffect(() => setSince(Date.now()), [turn]);
  useTick(1000);
  const sec = Math.max(0, Math.floor((Date.now() - since) / 1000));
  const label = awaiting ? "Ждёт разрешения" : toolTitle || (phase ? PHASE_LABEL[phase] : "Думаю");
  return (
    <>
      <div className="compact-side compact-left">
        <span key={label} className={`think-label${awaiting ? " is-awaiting" : ""}`} title={label}>
          {label}
        </span>
      </div>
      <div className="compact-center-spacer" aria-hidden="true" />
      <div className="compact-side compact-right">
        <span className="think-time">
          {Math.floor(sec / 60)}:{String(sec % 60).padStart(2, "0")}
        </span>
      </div>
    </>
  );
}

/** Resting state: centered amber eyes, left media cover+equalizer / focus ring / mission badge,
 *  and right countdown / TTS live stop indicator / job spinner. */
/** «TikTok 4:12» — the day's short-video minutes running out, or «закрыто». */
function VideoLeft({ w }: { w: Watching }) {
  const left = Math.max(0, w.limitSec - w.usedSec);
  const mm = Math.floor(left / 60);
  const ss = String(left % 60).padStart(2, "0");
  return (
    <span className={`compact-video${left < 60 ? " is-low" : ""}`} title={w.strict ?? `${w.label}: осталось ${mm} мин`}>
      <i style={{ ["--p" as string]: `${Math.min(100, (w.usedSec / Math.max(1, w.limitSec)) * 100)}%` }} />
      <Icon name="reels" size={11} />
      <span>{w.strict ? "закрыто" : `${mm}:${ss}`}</span>
    </span>
  );
}

/** The next calendar block inside the reminder window: «12 мин». */
function SoonRight({ u }: { u: Upcoming }) {
  return (
    <span className="compact-soon" title={`${u.event.title} · ${u.event.start.slice(11, 16)}`}>
      <Icon name="calendar" size={11} />
      {u.minutes} мин
    </span>
  );
}

/** XP just earned: «+15 XP» for a few seconds. */
function GainRight({ xp }: { xp: number }) {
  return (
    <span className="compact-xp" title="Опыт за полезное">
      <Icon name="spark" size={11} />+{xp} XP
    </span>
  );
}

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
  const watching = useMentor((s) => s.watching);
  const gain = useMentor((s) => s.gain);
  const soon = useAgenda((s) => s.soon);

  if (lip) return <div className="view view-compact" title={voiceError ?? undefined} />;

  const doneSteps = missionSteps.filter((x) => x.status === "done").length;

  if (agentActive && !ttsSpeaking) {
    return (
      <div className="view view-compact is-thinking">
        <ThinkingCompact />
      </div>
    );
  }

  return (
    <div className="view view-compact">
      <div className="compact-side compact-left">
        {watching ? (
          <VideoLeft w={watching} />
        ) : withMedia ? (
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
        ) : gain ? (
          <GainRight xp={gain.xp} />
        ) : running.length > 0 || agentActive ? (
          <span className="job-ring" aria-label="Джарвис работает" />
        ) : soon ? (
          <SoonRight u={soon} />
        ) : (
          <LowHeadset />
        )}
      </div>
    </div>
  );
}
