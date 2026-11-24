// Crisp 20px hardware-grade vector icons for the Workspace & Settings TTS Studio.
// Kept in `src/workspace/WorkspaceIcons.tsx` so `src/ui/Icon.tsx` (owned by ui-island)
// remains untouched and merges with zero conflicts.

import type { CSSProperties } from "react";

const glyphs = {
  bolt: <path d="M11.2 2.6 4.8 11h4.6l-.6 6.4 6.4-8.4h-4.6l.6-6.4Z" fill="currentColor" stroke="none" />,
  boltOutline: <path d="M11 2.8 4.8 11h4.6L9 17.2 15.2 9h-4.6Z" />,
  speaker: (
    <>
      <path d="M3.5 8.2v3.6a1 1 0 0 0 1 1h2.1l3.5 2.9V4.3L6.6 7.2H4.5a1 1 0 0 0-1 1Z" />
      <path d="M13 7.5a3.4 3.4 0 0 1 0 5" />
      <path d="M15.3 5.3a6.4 6.4 0 0 1 0 9.4" />
    </>
  ),
  speakerOff: (
    <>
      <path d="M3.5 8.2v3.6a1 1 0 0 0 1 1h2.1l3.5 2.9V4.3L6.6 7.2H4.5a1 1 0 0 0-1 1Z" />
      <path d="m13.2 8 3.6 4M16.8 8l-3.6 4" />
    </>
  ),
  copy: (
    <>
      <rect x="7" y="7" width="9.5" height="9.5" rx="2" />
      <path d="M13 7V5.2a1.8 1.8 0 0 0-1.8-1.8H5.2a1.8 1.8 0 0 0-1.8 1.8v6A1.8 1.8 0 0 0 5.2 13H7" />
    </>
  ),
  check: <path d="m4.6 10.4 3.5 3.4 7.3-7.6" />,
  plan: (
    <>
      <rect x="3.5" y="3.5" width="13" height="13" rx="2.5" />
      <path d="m6.8 8.2 1.4 1.4 2.8-2.8M6.8 12.6h6.4" />
    </>
  ),
  target: (
    <>
      <circle cx="10" cy="10" r="6.8" />
      <circle cx="10" cy="10" r="3.4" />
      <circle cx="10" cy="10" r="1" fill="currentColor" stroke="none" />
    </>
  ),
  sun: (
    <>
      <circle cx="10" cy="10" r="3.2" />
      <path d="M10 2.8v1.7M10 15.5v1.7M2.8 10h1.7M15.5 10h1.7M4.9 4.9l1.2 1.2M13.9 13.9l1.2 1.2M4.9 15.1l1.2-1.2M13.9 6.1l1.2-1.2" />
    </>
  ),
  window: (
    <>
      <rect x="3" y="4" width="14" height="12" rx="2.2" />
      <path d="M10 4v12M3 7.5h14" />
    </>
  ),
  cpu: (
    <>
      <rect x="5.5" y="5.5" width="9" height="9" rx="1.8" />
      <rect x="8" y="8" width="4" height="4" rx="0.8" />
      <path d="M8 3.2v2.3M12 3.2v2.3M8 14.5v2.3M12 14.5v2.3M3.2 8h2.3M3.2 12h2.3M14.5 8h2.3M14.5 12h2.3" />
    </>
  ),
  selection: (
    <>
      <path d="M4 6V4.8A1.3 1.3 0 0 1 5.3 3.5H7M13 3.5h1.7A1.3 1.3 0 0 1 16 4.8V6M16 14v1.2a1.3 1.3 0 0 1-1.3 1.3H13M7 16.5H5.3A1.3 1.3 0 0 1 4 15.2V14" />
      <path d="M7.2 8.2h5.6M7.2 11.8h3.8" />
    </>
  ),
  plus: <path d="M10 4.5v11M4.5 10h11" />,
  close: <path d="m5.8 5.8 8.4 8.4M14.2 5.8l-8.4 8.4" />,
  stop: <rect x="5.5" y="5.5" width="9" height="9" rx="2" fill="currentColor" stroke="none" />,
  play: <path d="M6.8 4.8v10.4a.8.8 0 0 0 1.2.7l8.2-5.2a.8.8 0 0 0 0-1.4L8 4.1a.8.8 0 0 0-1.2.7Z" fill="currentColor" stroke="none" />,
  chevronDown: <path d="m5.8 7.8 4.2 4.2 4.2-4.2" />,
  chevronRight: <path d="m7.8 5.8 4.2 4.2-4.2 4.2" />,
  shield: (
    <>
      <path d="M10 3.2 15.6 5.4v4.4c0 3.6-2.4 6.2-5.6 7.4-3.2-1.2-5.6-3.8-5.6-7.4V5.4L10 3.2Z" />
      <path d="M10 7.6v3.2M10 13.2v.1" />
    </>
  ),
  trash: (
    <path d="M4.5 6h11M8 6V4.4a.8.8 0 0 1 .8-.8h2.4a.8.8 0 0 1 .8.8V6M5.8 6l.6 9.1a1.2 1.2 0 0 0 1.2 1.1h4.8a1.2 1.2 0 0 0 1.2-1.1L14.2 6" />
  ),
  edit: (
    <path d="M13.6 3.8a1.6 1.6 0 0 1 2.3 2.3l-8.3 8.3-3.1.8.8-3.1 8.3-8.3Z" />
  ),
  gauge: (
    <>
      <path d="M4.2 14.2a7 7 0 1 1 11.6 0" />
      <path d="m10 10.5 3.2-3.2" />
      <circle cx="10" cy="10.5" r="1.2" fill="currentColor" stroke="none" />
    </>
  ),
  repeat: (
    <>
      <path d="M15.8 11a6 6 0 1 1-1.7-5.2" />
      <path d="M15.4 3.4v3.4H12" />
    </>
  ),
} as const;

export type WsIconName = keyof typeof glyphs;

export function WsIcon({
  name,
  size = 16,
  style,
  className,
}: {
  name: WsIconName;
  size?: number;
  style?: CSSProperties;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={style}
      className={className}
    >
      {glyphs[name]}
    </svg>
  );
}

export function routineIconName(icon: string): WsIconName {
  if (icon === "target" || icon === "focus") return "target";
  if (icon === "sun" || icon === "morning") return "sun";
  if (icon === "window" || icon === "layout") return "window";
  if (icon === "cpu" || icon === "diag") return "cpu";
  if (icon === "selection" || icon === "speaker") return "speaker";
  return "bolt";
}

/** Live animated 3-bar equalizer for active TTS playback. */
export function LiveAudioBars({ active = true, className = "" }: { active?: boolean; className?: string }) {
  return (
    <span className={`ws-audio-bars${active ? " is-active" : ""} ${className}`} aria-hidden="true">
      <i />
      <i />
      <i />
    </span>
  );
}
