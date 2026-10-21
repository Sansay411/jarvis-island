import { useEffect, useState } from "react";
import { Icon } from "../../ui/Icon";
import { errorMessage, ipc } from "../../lib/ipc";
import type { MediaAction, MediaState } from "../../lib/types";
import { useSystem } from "../../store/system";

/** Readable name for a GSMTC source AUMID ("Spotify.exe", "MSEdge", "…!App"). */
export function sourceLabel(aumid: string): string {
  if (!aumid) return "";
  let s = aumid.split("!")[0] ?? aumid;
  s = s.replace(/\.exe$/i, "");
  s = s.split("_")[0] ?? s;
  const tail = s.split(".").pop() ?? s;
  if (/^msedge$/i.test(tail)) return "Edge";
  if (/^zunemusic$/i.test(tail)) return "Медиаплеер";
  return tail.charAt(0).toUpperCase() + tail.slice(1);
}

function fmt(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m % 60).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

function livePosition(m: MediaState): number | null {
  if (m.positionMs === null || m.durationMs === null) return null;
  const drift = m.status === "playing" && m.positionSampledAt ? Date.now() - m.positionSampledAt : 0;
  return Math.min(m.durationMs, m.positionMs + Math.max(0, drift));
}

export function Artwork({ size, radius }: { size: number; radius: number }) {
  const media = useSystem((s) => s.media);
  const art = useSystem((s) => s.artwork);
  const url = art && media && art.trackKey === media.trackKey ? art.dataUrl : null;
  const dim = media?.status !== "playing";
  return (
    <div
      className="artwork"
      style={{ width: size, height: size, borderRadius: radius, opacity: dim ? 0.55 : 1 }}
      aria-hidden="true"
    >
      {url ? <img src={url} alt="" draggable={false} /> : <span>{sourceLabel(media?.sourceApp ?? "").charAt(0)}</span>}
    </div>
  );
}

export function MediaCard({ media }: { media: MediaState }) {
  // Re-render once a second while visible and playing to advance the clock.
  const [, tick] = useState(0);
  const [busy, setBusy] = useState<MediaAction | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (media.status !== "playing") return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [media.status]);

  const pos = livePosition(media);
  const frac = pos !== null && media.durationMs ? pos / media.durationMs : 0;
  const playing = media.status === "playing";

  const run = async (action: MediaAction) => {
    setBusy(action);
    setError(null);
    try {
      const out = await ipc.media.control(action);
      if (out.state.available) useSystem.setState({ media: out.state });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="media-card">
      <Artwork size={64} radius={13} />
      <div className="media-body">
        <div className="media-meta">
          <div className="media-title" title={media.title}>
            {media.title || "Без названия"}
          </div>
          <div className="media-artist" title={error ?? media.artist}>
            {error ?? [media.artist, sourceLabel(media.sourceApp)].filter(Boolean).join(" — ")}
          </div>
        </div>
        <div className="media-row">
          {pos !== null && media.durationMs ? (
            <div className="media-progress" aria-label={`${fmt(pos)} из ${fmt(media.durationMs)}`}>
              <span className="media-time">{fmt(pos)}</span>
              <div className="media-bar">
                <div className="media-bar-fill" style={{ transform: `scaleX(${frac})` }} />
              </div>
              <span className="media-time">{fmt(media.durationMs)}</span>
            </div>
          ) : (
            <div className="media-progress" />
          )}
          <div className="media-controls">
            <button
              className="icon-btn"
              aria-label="Предыдущий трек"
              disabled={!media.canPrevious || busy !== null}
              onClick={() => run("previous")}
            >
              <Icon name="previous" size={16} />
            </button>
            <button
              className="icon-btn icon-btn-primary"
              aria-label={playing ? "Пауза" : "Воспроизвести"}
              disabled={!media.canToggle || busy !== null}
              onClick={() => run("play_pause")}
            >
              <Icon name={playing ? "pause" : "play"} size={18} />
            </button>
            <button
              className="icon-btn"
              aria-label="Следующий трек"
              disabled={!media.canNext || busy !== null}
              onClick={() => run("next")}
            >
              <Icon name="next" size={16} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
