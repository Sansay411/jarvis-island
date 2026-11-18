// JARVIS icon set: 20px grid, 1.6px strokes, rounded joins. Drawn for this
// product so every glyph shares one optical weight with the eyes.

import type { CSSProperties } from "react";

const paths = {
  play: <path d="M7 4.8v10.4a.8.8 0 0 0 1.2.7l8.3-5.2a.8.8 0 0 0 0-1.4L8.2 4.1a.8.8 0 0 0-1.2.7Z" fill="currentColor" stroke="none" />,
  pause: (
    <>
      <rect x="5.5" y="4.5" width="3" height="11" rx="1.1" fill="currentColor" stroke="none" />
      <rect x="11.5" y="4.5" width="3" height="11" rx="1.1" fill="currentColor" stroke="none" />
    </>
  ),
  next: (
    <>
      <path d="M4.5 5.6v8.8a.7.7 0 0 0 1.1.6l6.4-4.4a.7.7 0 0 0 0-1.2L5.6 5a.7.7 0 0 0-1.1.6Z" fill="currentColor" stroke="none" />
      <rect x="13.5" y="5" width="2.2" height="10" rx="1" fill="currentColor" stroke="none" />
    </>
  ),
  previous: (
    <>
      <path d="M15.5 5.6v8.8a.7.7 0 0 1-1.1.6L8 10.6a.7.7 0 0 1 0-1.2L14.4 5a.7.7 0 0 1 1.1.6Z" fill="currentColor" stroke="none" />
      <rect x="4.3" y="5" width="2.2" height="10" rx="1" fill="currentColor" stroke="none" />
    </>
  ),
  volume: (
    <>
      <path d="M3.5 8v4a1 1 0 0 0 1 1h2.2l3.6 3V4L6.7 7H4.5a1 1 0 0 0-1 1Z" />
      <path d="M13.2 7.3a3.6 3.6 0 0 1 0 5.4" />
      <path d="M15.4 5.2a6.6 6.6 0 0 1 0 9.6" />
    </>
  ),
  muted: (
    <>
      <path d="M3.5 8v4a1 1 0 0 0 1 1h2.2l3.6 3V4L6.7 7H4.5a1 1 0 0 0-1 1Z" />
      <path d="m13 8 4 4M17 8l-4 4" />
    </>
  ),
  sun: (
    <>
      <circle cx="10" cy="10" r="3.2" />
      <path d="M10 2.8v1.6M10 15.6v1.6M2.8 10h1.6M15.6 10h1.6M4.9 4.9l1.1 1.1M14 14l1.1 1.1M4.9 15.1 6 14M14 6l1.1-1.1" />
    </>
  ),
  mic: (
    <>
      <rect x="7.2" y="2.8" width="5.6" height="9.4" rx="2.8" />
      <path d="M4.6 9.6a5.4 5.4 0 0 0 10.8 0M10 15v2.2" />
    </>
  ),
  send: <path d="M10 15.5v-11M5.5 9 10 4.5 14.5 9" />,
  stop: <rect x="5.5" y="5.5" width="9" height="9" rx="2" fill="currentColor" stroke="none" />,
  attach: <path d="m15.6 9.6-5.3 5.3a3.7 3.7 0 0 1-5.2-5.2l5.6-5.6a2.4 2.4 0 0 1 3.4 3.4l-5.5 5.5a1.1 1.1 0 0 1-1.6-1.6l5-5" />,
  chat: <path d="M4 5.5A2.5 2.5 0 0 1 6.5 3h7A2.5 2.5 0 0 1 16 5.5v5a2.5 2.5 0 0 1-2.5 2.5H9l-3.6 3v-3A2.5 2.5 0 0 1 4 10.5Z" />,
  collapse: <path d="m5.5 12.5 4.5-4.5 4.5 4.5" />,
  close: <path d="m5.5 5.5 9 9M14.5 5.5l-9 9" />,
  check: <path d="m4.8 10.4 3.4 3.3 7-7.4" />,
  alert: (
    <>
      <path d="M10 3.4 17 16H3Z" />
      <path d="M10 8.2v3.4M10 13.9v.1" />
    </>
  ),
  file: (
    <>
      <path d="M5.5 2.8h5.8l3.2 3.2v10.2a1 1 0 0 1-1 1h-8a1 1 0 0 1-1-1V3.8a1 1 0 0 1 1-1Z" />
      <path d="M11 2.8V6.3h3.5" />
    </>
  ),
  drop: (
    <>
      <path d="M10 3v9M6.2 8.6 10 12.4l3.8-3.8" />
      <path d="M3.5 13v1.7a1.8 1.8 0 0 0 1.8 1.8h9.4a1.8 1.8 0 0 0 1.8-1.8V13" />
    </>
  ),
  settings: (
    <>
      <circle cx="10" cy="10" r="2.3" />
      <path d="M10 3.4l.9 1.3 1.5-.2.5 1.5 1.4.7-.4 1.5 1 1.2-1 1.2.4 1.5-1.4.7-.5 1.5-1.5-.2-.9 1.3-.9-1.3-1.5.2-.5-1.5-1.4-.7.4-1.5-1-1.2 1-1.2-.4-1.5 1.4-.7.5-1.5 1.5.2Z" />
    </>
  ),
  window: (
    <>
      <rect x="3" y="4" width="14" height="12" rx="2" />
      <path d="M3 7.5h14" />
    </>
  ),
  terminal: (
    <>
      <rect x="2.8" y="3.8" width="14.4" height="12.4" rx="2" />
      <path d="m6 8 2.4 2L6 12M10.5 12.5h3.5" />
    </>
  ),
  globe: (
    <>
      <circle cx="10" cy="10" r="7" />
      <path d="M3 10h14M10 3c2 2.2 2.8 4.5 2.8 7s-.8 4.8-2.8 7c-2-2.2-2.8-4.5-2.8-7S8 5.2 10 3Z" />
    </>
  ),
  eye: (
    <>
      <path d="M2.8 10s2.6-5 7.2-5 7.2 5 7.2 5-2.6 5-7.2 5-7.2-5-7.2-5Z" />
      <circle cx="10" cy="10" r="2.2" />
    </>
  ),
  bolt: <path d="M11 2.8 4.8 11h4.6L9 17.2 15.2 9h-4.6Z" />,
  chevron: <path d="m8 5.5 4.5 4.5L8 14.5" />,
  chevronLeft: <path d="m12 5.5-4.5 4.5 4.5 4.5" />,
  timer: (
    <>
      <circle cx="10" cy="11" r="6" />
      <path d="M10 8v3.2l2 1.3M8.2 3h3.6" />
    </>
  ),
  clipboard: (
    <>
      <rect x="4.5" y="4.6" width="11" height="12.4" rx="2" />
      <path d="M7.6 4.6V4a1 1 0 0 1 1-1h2.8a1 1 0 0 1 1 1v.6M7.5 9h5M7.5 12h3.5" />
    </>
  ),
  search: (
    <>
      <circle cx="9" cy="9" r="5" />
      <path d="m13 13 3.5 3.5" />
    </>
  ),
  pin: (
    <>
      <path d="M7.2 3.8h5.6l-.8 4.3 2.3 2.5H5.7L8 8.1Z" />
      <path d="M10 10.6v5.6" />
    </>
  ),
  trash: <path d="M4.5 6h11M8 6V4.3a.8.8 0 0 1 .8-.8h2.4a.8.8 0 0 1 .8.8V6M5.8 6l.7 9.2a1.2 1.2 0 0 0 1.2 1.1h4.6a1.2 1.2 0 0 0 1.2-1.1L14.2 6M8.6 9v4.2M11.4 9v4.2" />,
  nook: (
    <>
      <path d="M5.2 16.5h9.6M7.6 16.5 9 11.2M12.8 4.4 7.4 7.8l3.8 3.8 3.4-5.4a1.2 1.2 0 0 0-1.8-1.8Z" />
    </>
  ),
  tray: (
    <>
      <path d="M3.6 11.2 5.4 4.8a1.4 1.4 0 0 1 1.35-1h6.5a1.4 1.4 0 0 1 1.35 1l1.8 6.4v3.4a1.6 1.6 0 0 1-1.6 1.6H5.2a1.6 1.6 0 0 1-1.6-1.6Z" />
      <path d="M3.8 11.4h3.6a2.6 2.6 0 0 0 5.2 0h3.6" />
    </>
  ),
  target: (
    <>
      <circle cx="10" cy="10" r="6.6" />
      <circle cx="10" cy="10" r="2.7" />
    </>
  ),
  grid: (
    <>
      <rect x="3.5" y="3.5" width="5.2" height="5.2" rx="1.2" />
      <rect x="11.3" y="3.5" width="5.2" height="5.2" rx="1.2" />
      <rect x="3.5" y="11.3" width="5.2" height="5.2" rx="1.2" />
      <rect x="11.3" y="11.3" width="5.2" height="5.2" rx="1.2" />
    </>
  ),
  plus: <path d="M10 4.5v11M4.5 10h11" />,
  calendar: (
    <>
      <rect x="3.4" y="4.6" width="13.2" height="11.8" rx="2.2" />
      <path d="M3.4 8.2h13.2M7 3v2.6M13 3v2.6" />
    </>
  ),
  speaker: (
    <>
      <path d="M3.5 8v4a1 1 0 0 0 1 1h2.2l3.6 3V4L6.7 7H4.5a1 1 0 0 0-1 1Z" />
      <path d="M13.4 7.5a3.5 3.5 0 0 1 0 5" />
    </>
  ),
  folder: <path d="M3.2 6a1.6 1.6 0 0 1 1.6-1.6h3.4l1.6 1.8h5.4A1.6 1.6 0 0 1 16.8 7.8v6.6a1.6 1.6 0 0 1-1.6 1.6H4.8a1.6 1.6 0 0 1-1.6-1.6Z" />,
  copy: (
    <>
      <rect x="7" y="7" width="9.2" height="9.2" rx="1.8" />
      <path d="M13 7V5.4a1.6 1.6 0 0 0-1.6-1.6H5.4a1.6 1.6 0 0 0-1.6 1.6v6A1.6 1.6 0 0 0 5.4 13H7" />
    </>
  ),
  music: (
    <>
      <path d="M8 14.5V5.2l7.5-1.6v9.4" />
      <circle cx="6.2" cy="14.6" r="2" />
      <circle cx="13.7" cy="13.2" r="2" />
    </>
  ),
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, size = 18, style, className }: { name: IconName; size?: number; style?: CSSProperties; className?: string }) {
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
      {paths[name]}
    </svg>
  );
}
