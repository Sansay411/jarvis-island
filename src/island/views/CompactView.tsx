import { hasActiveMedia, useSystem } from "../../store/system";
import { useRunningJobs } from "../../store/jobs";
import { Artwork } from "./MediaCard";
import { useIsland } from "../../store/island";
import { useVoice } from "../../store/voice";
import { useCountdown, useNextTimer } from "../../store/reminders";

function TimerLeft({ dueAt, text }: { dueAt: string; text: string }) {
  const left = useCountdown(dueAt);
  return <span className="compact-timer" title={text ? `Таймер «${text}»` : "Таймер"}>{left}</span>;
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
  if (lip) return <div className="view view-compact" title={voiceError??undefined}/>;
  return (
    <div className="view view-compact">
      <div className="compact-slot compact-left">{withMedia && <Artwork size={20} radius={6} />}</div>
      <div className="compact-slot compact-right" title={voiceError??running.map((j) => j.title).join("\n")}>
        {voiceError && <span className="voice-unavailable" role="status" aria-label="Отклик на имя временно недоступен"/>}
        {timer ? <TimerLeft dueAt={timer.dueAt} text={timer.text} /> : running.length > 0 && <span className="job-ring" aria-label={`Фоновых задач: ${running.length}`} />}
      </div>
    </div>
  );
}
