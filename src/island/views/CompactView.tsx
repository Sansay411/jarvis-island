import { hasActiveMedia, useSystem } from "../../store/system";
import { useRunningJobs } from "../../store/jobs";
import { Artwork } from "./MediaCard";
import { useIsland } from "../../store/island";

/** Resting state: the eyes, a tiny cover when something plays, and a ring
 *  while background work (coding agents) is running. */
export function CompactView() {
  const media = useSystem((s) => s.media);
  const running = useRunningJobs();
  const withMedia = hasActiveMedia(media);
  const lip = useIsland((s) => s.topCovered);
  if (lip) return <div className="view view-compact" />;
  return (
    <div className="view view-compact">
      <div className="compact-slot compact-left">{withMedia && <Artwork size={20} radius={6} />}</div>
      <div className="compact-slot compact-right" title={running.map((j) => j.title).join("\n")}>
        {running.length > 0 && <span className="job-ring" aria-label={`Фоновых задач: ${running.length}`} />}
      </div>
    </div>
  );
}
