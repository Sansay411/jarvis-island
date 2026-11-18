import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { Icon } from "../../ui/Icon";
import { errorMessage, ipc, isTauri } from "../../lib/ipc";
import type { MediaAction, MediaState } from "../../lib/types";
import { useSystem } from "../../store/system";

export type MediaSourceKind = "spotify" | "apple" | "yandex" | "telegram" | "youtube" | "browser" | "generic";

export function detectSourceKind(aumid: string): MediaSourceKind {
  const s = (aumid || "").toLowerCase();
  if (s.includes("spotify")) return "spotify";
  if (s.includes("applemusic") || s.includes("itunes") || s.includes("apple")) return "apple";
  if (s.includes("yandex") || s.includes("яндекс")) return "yandex";
  if (s.includes("telegram")) return "telegram";
  if (s.includes("youtube")) return "youtube";
  if (
    s.includes("chrome") ||
    s.includes("msedge") ||
    s.includes("edge") ||
    s.includes("firefox") ||
    s.includes("brave") ||
    s.includes("opera") ||
    s.includes("vivaldi") ||
    s.includes("arc")
  ) {
    return "browser";
  }
  return "generic";
}

/** Readable name for a GSMTC source AUMID ("Spotify.exe", "MSEdge", "…!App"). */
export function sourceLabel(aumid: string): string {
  if (!aumid) return "";
  const kind = detectSourceKind(aumid);
  if (kind === "spotify") return "Spotify";
  if (kind === "apple") return "Apple Music";
  if (kind === "yandex") return "Яндекс Музыка";
  if (kind === "telegram") return "Telegram";
  if (kind === "youtube") return "YouTube";
  let s = aumid.split("!")[0] ?? aumid;
  s = s.replace(/\.exe$/i, "");
  s = s.split("_")[0] ?? s;
  const tail = s.split(".").pop() ?? s;
  if (/^msedge$/i.test(tail)) return "Edge";
  if (/^chrome$/i.test(tail)) return "Chrome";
  if (/^zunemusic$/i.test(tail)) return "Медиаплеер";
  return tail.charAt(0).toUpperCase() + tail.slice(1);
}

/** Hardware squircle app badge anchored on album art (NotchNook Refs 2, 3, 4). */
export function SourceBadge({ aumid, size = 19 }: { aumid: string; size?: number }) {
  const kind = detectSourceKind(aumid);
  const label = sourceLabel(aumid);
  return (
    <span className={`source-badge is-${kind}`} style={{ width: size, height: size }} title={label || "Медиаплеер"} aria-hidden="true">
      {kind === "spotify" && (
        <svg viewBox="0 0 16 16" width={size * 0.68} height={size * 0.68} fill="none">
          <path d="M3.3 6.1c3.1-.9 6.7-.7 9.5.8" stroke="#040406" strokeWidth="1.55" strokeLinecap="round" />
          <path d="M4.1 8.7c2.5-.7 5.3-.5 7.6.7" stroke="#040406" strokeWidth="1.35" strokeLinecap="round" />
          <path d="M4.8 11.1c1.9-.5 4-.3 5.7.5" stroke="#040406" strokeWidth="1.2" strokeLinecap="round" />
        </svg>
      )}
      {kind === "apple" && (
        <svg viewBox="0 0 16 16" width={size * 0.66} height={size * 0.66} fill="none">
          <path d="M6.4 11.4V4.7l5.2-1.1v6.8" stroke="#fff" strokeWidth="1.45" strokeLinecap="round" strokeLinejoin="round" />
          <circle cx="5" cy="11.6" r="1.5" fill="#fff" />
          <circle cx="10.2" cy="10.5" r="1.5" fill="#fff" />
        </svg>
      )}
      {kind === "yandex" && (
        <svg viewBox="0 0 16 16" width={size * 0.68} height={size * 0.68} fill="none">
          <path d="M8 2.4v11.2M3.2 5.2l9.6 5.6M12.8 5.2 3.2 10.8" stroke="#08080a" strokeWidth="1.55" strokeLinecap="round" />
        </svg>
      )}
      {kind === "telegram" && (
        <svg viewBox="0 0 16 16" width={size * 0.66} height={size * 0.66} fill="none">
          <path d="M12.9 3.6 2.8 7.5l3.3 1.2 1.3 3.5 2-1.9 2.5 1.8 1-8.5Z" fill="#fff" />
        </svg>
      )}
      {(kind === "youtube" || kind === "browser") && (
        <svg viewBox="0 0 16 16" width={size * 0.64} height={size * 0.64} fill="none">
          <path d="M6 4.6v6.8L11.6 8 6 4.6Z" fill="#fff" />
        </svg>
      )}
      {kind === "generic" && (
        <svg viewBox="0 0 16 16" width={size * 0.64} height={size * 0.64} fill="none">
          <path d="M6.6 11.3V4.8l4.8-1v6.5" stroke="#fff" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          <circle cx="5.3" cy="11.5" r="1.4" fill="#fff" />
          <circle cx="10.1" cy="10.5" r="1.4" fill="#fff" />
        </svg>
      )}
    </span>
  );
}

export function EqualizerBars({ playing, accent = "default" }: { playing: boolean; accent?: "default" | "amber" | "cobalt" }) {
  return (
    <span className={`eq-bars${playing ? " is-playing" : ""} tone-${accent}`} aria-hidden="true">
      <span className="eq-bar b1" />
      <span className="eq-bar b2" />
      <span className="eq-bar b3" />
    </span>
  );
}

function fmt(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m % 60).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export function livePosition(m: MediaState): number | null {
  if (m.positionMs === null || m.durationMs === null) return null;
  const drift = m.status === "playing" && m.positionSampledAt ? Date.now() - m.positionSampledAt : 0;
  return Math.min(m.durationMs, m.positionMs + Math.max(0, drift));
}

export function Artwork({ size, radius, showBadge = false }: { size: number; radius: number; showBadge?: boolean }) {
  const media = useSystem((s) => s.media);
  const art = useSystem((s) => s.artwork);
  const url = art && media && art.trackKey === media.trackKey ? art.dataUrl : null;
  const dim = media?.status !== "playing";

  return (
    <div className="artwork-wrap" style={{ width: size, height: size }}>
      <div
        className="artwork"
        style={{ width: size, height: size, borderRadius: radius, opacity: dim ? 0.68 : 1 }}
        aria-hidden="true"
      >
        {url ? (
          <img src={url} alt="" draggable={false} />
        ) : (
          <Icon name="music" size={Math.max(11, Math.round(size * 0.36))} />
        )}
      </div>
      {showBadge && media && <SourceBadge aumid={media.sourceApp} size={Math.max(16, Math.round(size * 0.34))} />}
    </div>
  );
}

/** Scrubbable timeline bar matching Reference 5 (`1:42 [====●---] -0:24`). */
function ScrubTimeline({ posMs, durationMs }: { posMs: number; durationMs: number }) {
  const trackRef = useRef<HTMLDivElement>(null);
  const [dragMs, setDragMs] = useState<number | null>(null);
  const seekMedia = useSystem((s) => s.seekMedia);

  const effectiveMs = dragMs ?? posMs;
  const frac = durationMs > 0 ? Math.min(1, Math.max(0, effectiveMs / durationMs)) : 0;
  const remainMs = Math.max(0, durationMs - effectiveMs);

  const msFromPointer = (clientX: number) => {
    const r = trackRef.current?.getBoundingClientRect();
    if (!r || r.width <= 0) return posMs;
    const ratio = Math.min(1, Math.max(0, (clientX - r.left) / r.width));
    return Math.round(ratio * durationMs);
  };

  const onDown = (e: PointerEvent<HTMLDivElement>) => {
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragMs(msFromPointer(e.clientX));
  };

  const onMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    setDragMs(msFromPointer(e.clientX));
  };

  const onUp = (e: PointerEvent<HTMLDivElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    const target = msFromPointer(e.clientX);
    setDragMs(null);
    void seekMedia(target);
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = 5000;
    if (e.key === "ArrowRight") {
      e.preventDefault();
      void seekMedia(Math.min(durationMs, posMs + step));
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      void seekMedia(Math.max(0, posMs - step));
    }
  };

  return (
    <div className="media-scrub-row" aria-label={`${fmt(effectiveMs)} из ${fmt(durationMs)}`}>
      <span className="media-time">{fmt(effectiveMs)}</span>
      <div
        ref={trackRef}
        className={`media-scrub-track${dragMs !== null ? " is-dragging" : ""}`}
        role="slider"
        tabIndex={0}
        aria-label="Позиция трека"
        aria-valuemin={0}
        aria-valuemax={Math.round(durationMs / 1000)}
        aria-valuenow={Math.round(effectiveMs / 1000)}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onKeyDown={onKey}
      >
        <div className="media-scrub-fill" style={{ width: `${(frac * 100).toFixed(2)}%` }} />
        <span className="media-scrub-thumb" style={{ left: `${(frac * 100).toFixed(2)}%` }} />
      </div>
      <span className="media-time is-remaining">-{fmt(remainMs)}</span>
    </div>
  );
}

export function MediaCard({ media }: { media: MediaState }) {
  const [, tick] = useState(0);
  const [busy, setBusy] = useState<MediaAction | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (media.status !== "playing") return;
    const id = window.setInterval(() => tick((n) => n + 1), 500);
    return () => window.clearInterval(id);
  }, [media.status]);

  const pos = livePosition(media);
  const playing = media.status === "playing";

  const run = async (action: MediaAction) => {
    if (!isTauri()) {
      useSystem.setState((s) => {
        if (!s.media) return s;
        if (action === "play_pause") {
          const nextStatus = s.media.status === "playing" ? "paused" : "playing";
          return {
            media: {
              ...s.media,
              status: nextStatus,
              positionMs: livePosition(s.media) ?? s.media.positionMs,
              positionSampledAt: Date.now(),
            },
          };
        }
        return {
          media: {
            ...s.media,
            positionMs: 0,
            positionSampledAt: Date.now(),
          },
        };
      });
      return;
    }
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

  const subtitle = error ?? ([media.artist, media.album || sourceLabel(media.sourceApp)].filter(Boolean).join(" · ") || sourceLabel(media.sourceApp));

  return (
    <div className="nook-media">
      <div className="nook-media-top">
        <Artwork size={54} radius={13} showBadge />
        <div className="nook-media-info">
          <div className="media-title-row">
            <div className="media-title" title={media.title}>
              {media.title || "Без названия"}
            </div>
            <EqualizerBars playing={playing} />
          </div>
          <div className="media-artist" title={subtitle}>
            {subtitle}
          </div>
          <div className="nook-transport">
            <button
              className="transport-btn"
              aria-label="Предыдущий трек"
              disabled={!media.canPrevious || busy !== null}
              onClick={() => void run("previous")}
            >
              <Icon name="previous" size={14} />
            </button>
            <button
              className="transport-btn is-primary"
              aria-label={playing ? "Пауза" : "Воспроизвести"}
              disabled={!media.canToggle || busy !== null}
              onClick={() => void run("play_pause")}
            >
              <Icon name={playing ? "pause" : "play"} size={16} />
            </button>
            <button
              className="transport-btn"
              aria-label="Следующий трек"
              disabled={!media.canNext || busy !== null}
              onClick={() => void run("next")}
            >
              <Icon name="next" size={14} />
            </button>
          </div>
        </div>
      </div>

      {pos !== null && media.durationMs && media.durationMs > 0 ? (
        <ScrubTimeline posMs={pos} durationMs={media.durationMs} />
      ) : null}
    </div>
  );
}
