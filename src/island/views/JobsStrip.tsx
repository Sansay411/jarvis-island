import { Icon } from "../../ui/Icon";
import { cancelJob, useRunningJobs } from "../../store/jobs";

/** Running background jobs with their latest output line. */
export function JobsStrip() {
  const running = useRunningJobs();
  if (running.length === 0) return null;
  return (
    <div className="jobs-strip">
      {running.slice(0, 2).map((j) => (
        <div key={j.id} className="job-row">
          <span className="job-ring" aria-hidden="true" />
          <span className="job-title">{j.title}</span>
          <span className="job-line">{j.tail[j.tail.length - 1] ?? "Запускается…"}</span>
          <button className="icon-btn" aria-label={`Остановить ${j.title}`} onClick={() => void cancelJob(j.id)}>
            <Icon name="stop" size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
