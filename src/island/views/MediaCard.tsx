import { useEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent, type PointerEvent, type WheelEvent } from "react";
import { Icon } from "../../ui/Icon";
import { errorMessage, ipc, isTauri } from "../../lib/ipc";
import type { MediaAction, MediaState } from "../../lib/types";
import { getLastTrack, onLastTrack, useSystem } from "../../store/system";

export type MediaSourceKind =
  | "spotify"
  | "apple"
  | "yandex"
  | "telegram"
  | "youtube"
  | "soundcloud"
  | "vk"
  | "twitch"
  | "browser"
  | "generic";

const SITE_KINDS: Record<string, MediaSourceKind> = {
  youtube: "youtube",
  yandex: "yandex",
  spotify: "spotify",
  soundcloud: "soundcloud",
  vk: "vk",
  twitch: "twitch",
};

/** What plays: the site when a browser told us (window title), else the app. */
export function mediaSourceKind(aumid: string, site?: string): MediaSourceKind {
  const bySite = site ? SITE_KINDS[site] : undefined;
  return bySite ?? detectSourceKind(aumid);
}

/** Readable source name, the site first («YouTube», not «Chrome»). */
export function mediaSourceName(aumid: string, site?: string): string {
  const kind = mediaSourceKind(aumid, site);
  if (kind === "soundcloud") return "SoundCloud";
  if (kind === "vk") return "VK Музыка";
  if (kind === "twitch") return "Twitch";
  if (site && SITE_KINDS[site]) {
    if (kind === "yandex") return "Яндекс Музыка";
    if (kind === "spotify") return "Spotify";
    if (kind === "youtube") return "YouTube";
  }
  return sourceLabel(aumid);
}

// The playing app's real icon, asked from Windows once per app id.
const iconCache = new Map<string, string | null>();
const iconWaiters = new Map<string, Promise<string | null>>();
export function useAppIcon(aumid: string | undefined): string | null {
  const id = aumid ?? "";
  const [icon, setIcon] = useState<string | null>(() => iconCache.get(id) ?? null);
  useEffect(() => {
    if (!id) return setIcon(null);
    if (iconCache.has(id)) return setIcon(iconCache.get(id) ?? null);
    let alive = true;
    let p = iconWaiters.get(id);
    if (!p) {
      p = ipc.media
        .appIcon(id)
        .catch(() => null)
        .then((v) => {
          iconCache.set(id, v);
          iconWaiters.delete(id);
          return v;
        });
      iconWaiters.set(id, p);
    }
    void p.then((v) => alive && setIcon(v));
    return () => {
      alive = false;
    };
  }, [id]);
  return icon;
}

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

/** Yandex Music's yellow burst, for when Windows has no icon to give. */
const STARBURST =
  "M8.00 0.80 L8.46 6.16 L10.35 3.59 L9.26 6.58 L13.57 4.10 L9.76 7.27 L12.53 7.20 L9.90 8.13 L14.09 9.98 L9.58 9.06 L11.34 11.98 L8.80 9.72 L9.22 14.89 L7.83 9.89 L6.36 12.51 L6.94 9.58 L3.39 12.15 L6.31 8.86 L2.72 9.12 L6.10 7.87 L1.42 5.61 L6.42 6.94 L4.79 4.43 L7.32 6.23Z";

/** Hardware squircle app badge anchored on album art (NotchNook Refs 2, 3, 4). */
export function SourceBadge({ aumid, site, size = 19 }: { aumid: string; site?: string; size?: number }) {
  const kind = mediaSourceKind(aumid, site);
  const label = mediaSourceName(aumid, site);
  const appIcon = useAppIcon(aumid);
  // A known site wins (YouTube in Chrome shows YouTube, not Chrome); for an
  // app the real icon Windows has for it; the drawn marks are the fallback.
  const siteKnown = !!(site && SITE_KINDS[site]);
  if (!siteKnown && appIcon) {
    return (
      <span className="source-badge is-app-icon" style={{ width: size, height: size }} title={label || "Медиаплеер"} aria-hidden="true">
        <img src={appIcon} alt="" draggable={false} />
      </span>
    );
  }
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
        <svg viewBox="0 0 16 16" width={size * 0.78} height={size * 0.78}>
          <path d={STARBURST} fill="#ffd21f" />
        </svg>
      )}
      {kind === "soundcloud" && (
        <svg viewBox="0 0 16 16" width={size * 0.72} height={size * 0.72} fill="#fff">
          <path d="M6.6 5.4c.6-.5 1.4-.8 2.3-.8 1.8 0 3.2 1.3 3.4 3 .2-.1.5-.1.7-.1 1 0 1.8.8 1.8 1.8s-.8 1.8-1.8 1.8H6.6V5.4Z" />
          <rect x="5.1" y="6.4" width="0.9" height="4.7" rx=".45" />
          <rect x="3.6" y="7.2" width="0.9" height="3.9" rx=".45" />
          <rect x="2.1" y="8.2" width="0.9" height="2.9" rx=".45" />
        </svg>
      )}
      {kind === "vk" && (
        <svg viewBox="0 0 16 16" width={size * 0.7} height={size * 0.7} fill="#fff">
          <path d="M8.5 11.6C4.6 11.6 2.4 8.9 2.3 4.5h2c.1 3.2 1.5 4.6 2.6 4.9V4.5h1.9v2.8c1.1-.1 2.3-1.4 2.7-2.8h1.9c-.3 1.7-1.6 3-2.5 3.5.9.4 2.4 1.6 2.9 3.6h-2.1c-.4-1.4-1.6-2.4-2.9-2.6v2.6h-.3Z" />
        </svg>
      )}
      {kind === "twitch" && (
        <svg viewBox="0 0 16 16" width={size * 0.66} height={size * 0.66} fill="#fff">
          <path d="M4 2.5 3 5v8h2.6v1.5h1.5L8.6 13h2.2L13 10.8V2.5H4Zm8 7.8-1.5 1.5H8.2l-1.3 1.3v-1.3H5V3.5h7v6.8Z" />
          <path d="M9.6 5.2h1v3h-1zM7 5.2h1v3H7z" />
        </svg>
      )}
      {kind === "telegram" && (
        <svg viewBox="0 0 16 16" width={size * 0.66} height={size * 0.66} fill="none">
          <path d="M12.9 3.6 2.8 7.5l3.3 1.2 1.3 3.5 2-1.9 2.5 1.8 1-8.5Z" fill="#fff" />
        </svg>
      )}
      {kind === "youtube" && (
        <svg viewBox="0 0 16 16" width={size * 0.6} height={size * 0.6} fill="none">
          <path d="M6.2 5v6l5-3-5-3Z" fill="#fff" />
        </svg>
      )}
      {kind === "browser" && (
        <svg viewBox="0 0 16 16" width={size * 0.66} height={size * 0.66} fill="none" stroke="#fff" strokeWidth="1.2">
          <circle cx="8" cy="8" r="5.4" />
          <path d="M2.6 8h10.8M8 2.6c1.6 1.5 2.3 3.3 2.3 5.4S9.6 11.9 8 13.4C6.4 11.9 5.7 10.1 5.7 8S6.4 4.1 8 2.6Z" />
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

export function Artwork({
  size,
  radius,
  showBadge = false,
  nudge = null,
}: {
  size: number;
  radius: number;
  showBadge?: boolean;
  /** A short push sideways when the user skips a track. */
  nudge?: "next" | "previous" | null;
}) {
  const media = useSystem((s) => s.media);
  const art = useSystem((s) => s.artwork);
  const url = art && media && art.trackKey === media.trackKey ? art.dataUrl : null;
  const playing = media?.status === "playing";
  const appIcon = useAppIcon(media?.sourceApp);

  return (
    <div
      className={`artwork-wrap${playing ? " is-playing" : " is-paused"}${nudge ? ` is-nudge-${nudge}` : ""}`}
      style={{ width: size, height: size, ["--r" as string]: `${radius}px` }}
    >
      <div className="artwork" style={{ width: size, height: size, borderRadius: radius }} aria-hidden="true">
        {url ? (
          <img key={media?.trackKey} className="art-swap" src={url} alt="" draggable={false} />
        ) : appIcon && size >= 28 ? (
          // No cover from the app: its own logo, centred, instead of a note.
          <img className="artwork-app-icon" src={appIcon} alt="" draggable={false} style={{ width: "46%", height: "46%" }} />
        ) : (
          <Icon name="music" size={Math.max(11, Math.round(size * 0.36))} />
        )}
      </div>
      {showBadge && media && <SourceBadge aumid={media.sourceApp} site={media.site} size={Math.max(14, Math.round(size * 0.26))} />}
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
  const [nudge, setNudge] = useState<"next" | "previous" | null>(null);
  const wheelVolume = useWheelVolume();

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
    if (action === "next" || action === "previous") {
      setNudge(action);
      window.setTimeout(() => setNudge(null), 420);
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

  const source = mediaSourceName(media.sourceApp, media.site);
  const subtitle = error ?? ([media.artist, media.album || source].filter(Boolean).join(" · ") || source);

  return (
    <div className={`nook-media is-live${playing ? " is-playing" : ""}`} onWheel={wheelVolume.onWheel}>
      {wheelVolume.pill}
      <div className="nook-media-top">
        <Artwork size={68} radius={14} showBadge nudge={nudge} />
        <div className="nook-media-info" key={media.trackKey}>
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

/** Nothing is playing: the player stays with the last track Windows showed
 *  (dimmed, ready to go on). «Продолжить» asks that player to play again —
 *  only the system player, never a site in the browser. */
export function IdlePlayer() {
  const last = useSyncExternalStore(onLastTrack, getLastTrack);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const go = async () => {
    setBusy(true);
    setNote(null);
    try {
      const out = await ipc.media.control("play");
      if (out.state.available) useSystem.setState({ media: out.state });
      else setNote("Плеер закрыт — включите музыку в нём");
    } catch (e) {
      setNote(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  // A slim row, never a big empty card: the last track if Windows showed one,
  // otherwise just a quiet «музыка» toggle.
  const sub = note ?? (last ? last.artist || sourceLabel(last.sourceApp) : null);
  return (
    <button
      type="button"
      className="idle-track"
      disabled={busy}
      onClick={() => void go()}
      title={last ? "Продолжить" : "Включить музыку"}
    >
      <span className="idle-track-art" aria-hidden="true">
        {last?.art ? <img src={last.art} alt="" draggable={false} /> : <Icon name="music" size={12} />}
      </span>
      <span className="idle-track-text">
        <span className="idle-track-title">{last ? last.title || "Без названия" : "Музыка"}</span>
        {sub && <span className="idle-track-sub">{sub}</span>}
      </span>
      <span className="idle-track-play" aria-hidden="true">
        <Icon name="play" size={10} />
      </span>
    </button>
  );
}

/** Wheel over the player = system volume, with a small pill that says how loud. */
function useWheelVolume() {
  const volume = useSystem((s) => s.audio?.volume ?? null);
  const setVolume = useSystem((s) => s.setVolume);
  const [shown, setShown] = useState(false);
  const hide = useRef<number | undefined>(undefined);
  const acc = useRef(0);
  const onWheel = (e: WheelEvent<HTMLDivElement>) => {
    if (volume === null) return;
    acc.current += e.deltaY;
    if (Math.abs(acc.current) < 40) return;
    const step = acc.current < 0 ? 4 : -4;
    acc.current = 0;
    void setVolume(volume + step);
    setShown(true);
    window.clearTimeout(hide.current);
    hide.current = window.setTimeout(() => setShown(false), 1100);
  };
  useEffect(() => () => window.clearTimeout(hide.current), []);
  const v = volume ?? 0;
  const pill = (
    <div className={`media-volume-pill${shown ? " is-shown" : ""}`} aria-hidden={!shown}>
      <Icon name={v === 0 ? "muted" : "volume"} size={12} />
      <span className="media-volume-bar">
        <span style={{ width: `${v}%` }} />
      </span>
      <span className="media-volume-num">{v}</span>
    </div>
  );
  return { onWheel, pill };
}
