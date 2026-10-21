import { hasActiveMedia, useSystem } from "../../store/system";
import { Artwork } from "./MediaCard";

/** Resting state: just the eyes, plus a tiny cover when something plays. */
export function CompactView() {
  const media = useSystem((s) => s.media);
  const withMedia = hasActiveMedia(media);
  return (
    <div className="view view-compact">
      <div className="compact-slot compact-left">{withMedia && <Artwork size={20} radius={6} />}</div>
      <div className="compact-slot compact-right" />
    </div>
  );
}
