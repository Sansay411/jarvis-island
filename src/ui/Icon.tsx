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
      <rect x="7.25" y="3" width="5.5" height="9" rx="2.75" />
      <path d="M4.75 9.5a5.25 5.25 0 0 0 10.5 0M10 14.75V17" />
    </>
  ),
  send: <path d="M10 15.5v-11M5.5 9 10 4.5 14.5 9" />,
  plane: (
    <>
      <path d="M16.6 3.4 3.6 8.6a.5.5 0 0 0 0 .9l4.9 2 2 4.9a.5.5 0 0 0 .9 0l5.2-13a.3.3 0 0 0 0 0Z" />
      <path d="m8.5 11.5 3.3-3.3" />
    </>
  ),
  stop: <rect x="5.5" y="5.5" width="9" height="9" rx="2" fill="currentColor" stroke="none" />,
  attach: <path d="m15.6 9.6-5.3 5.3a3.7 3.7 0 0 1-5.2-5.2l5.6-5.6a2.4 2.4 0 0 1 3.4 3.4l-5.5 5.5a1.1 1.1 0 0 1-1.6-1.6l5-5" />,
  chat: <path d="M10 3.5c3.9 0 6.5 2.6 6.5 5.8S13.9 15 10 15c-.9 0-1.7-.1-2.5-.4L4 16l.9-2.8c-.9-1-1.4-2.3-1.4-3.9C3.5 6.1 6.1 3.5 10 3.5Z" />,
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
      <path d="M3.5 6.5H10M14 6.5h2.5M3.5 13.5H6M10 13.5h6.5" />
      <circle cx="12" cy="6.5" r="2" />
      <circle cx="8" cy="13.5" r="2" />
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
  pinned: (
    <>
      <path d="M7.2 3.8h5.6l-.8 4.3 2.3 2.5H5.7L8 8.1Z" fill="currentColor" />
      <path d="M10 10.6v5.6" />
    </>
  ),
  headphones: (
    <>
      <path d="M3.8 12.2V10a6.2 6.2 0 0 1 12.4 0v2.2" />
      <rect x="3" y="11.2" width="3.7" height="5.6" rx="1.5" />
      <rect x="13.3" y="11.2" width="3.7" height="5.6" rx="1.5" />
    </>
  ),
  budLeft: (
    <>
      <path d="M11.65 7.17 12.3 15.3a1.15 1.15 0 0 1-2.3.1L9.53 9.7A3.3 3.3 0 1 1 11.65 7.17Z" />
      <circle cx="7.3" cy="6.1" r=".75" fill="currentColor" stroke="none" />
    </>
  ),
  budRight: (
    <g transform="matrix(-1 0 0 1 20 0)">
      <path d="M11.65 7.17 12.3 15.3a1.15 1.15 0 0 1-2.3.1L9.53 9.7A3.3 3.3 0 1 1 11.65 7.17Z" />
      <circle cx="7.3" cy="6.1" r=".75" fill="currentColor" stroke="none" />
    </g>
  ),
  budCase: (
    <>
      <rect x="3.4" y="5.4" width="13.2" height="10.4" rx="3.4" />
      <path d="M3.6 8.9h12.8" />
      <circle cx="10" cy="12.5" r=".8" fill="currentColor" stroke="none" />
    </>
  ),
  keyboard: (
    <>
      <rect x="2.6" y="5.4" width="14.8" height="9.2" rx="2" />
      <path d="M5.6 8.3h.01M8.2 8.3h.01M10.8 8.3h.01M13.4 8.3h.01M6.2 11.6h7.6" />
    </>
  ),
  mouse: (
    <>
      <rect x="5.6" y="3" width="8.8" height="14" rx="4.4" />
      <path d="M10 5.6v2.4" />
    </>
  ),
  phone: (
    <>
      <rect x="6" y="2.8" width="8" height="14.4" rx="2.2" />
      <path d="M9 14.6h2" />
    </>
  ),
  gamepad: (
    <>
      <path d="M6.3 6.3h7.4a3.6 3.6 0 0 1 3.5 2.8l.8 3.7a2 2 0 0 1-3.4 1.8l-1.5-1.5H6.9l-1.5 1.5A2 2 0 0 1 2 12.8l.8-3.7a3.6 3.6 0 0 1 3.5-2.8Z" />
      <path d="M6.8 8.6v2.4M5.6 9.8H8M12.6 9.1h.01M14.1 10.5h.01" />
    </>
  ),
  watch: (
    <>
      <rect x="5.8" y="5.4" width="8.4" height="9.2" rx="2.4" />
      <path d="M7.6 5.4 8.2 3h3.6l.6 2.4M7.6 14.6l.6 2.4h3.6l.6-2.4" />
    </>
  ),
  laptop: (
    <>
      <rect x="4.2" y="4.6" width="11.6" height="8" rx="1.4" />
      <path d="M2.8 15.4h14.4" />
    </>
  ),
  bluetooth: <path d="m6 6.6 8 6.4-4 3.2V3.8l4 3.2-8 6.4" />,
  // Growth: the mentor, «Перезагрузка» and the calendar on the island.
  flame: (
    <>
      <path d="M10 17.5a5.5 5.5 0 0 1-5.5-5.5c0-2.4 1.5-4 2.9-5.4.2 1.4.8 2.3 1.8 2.8-.1-2.9 1.1-5 3.3-6.4.1 2.3 1.1 3.7 2.3 5.1.5.8.7 2.4.7 3.9a5.5 5.5 0 0 1-5.5 5.5Z" />
      <path d="M10 17.5a2.3 2.3 0 0 1-2.3-2.3c0-1.3.9-2.2 2.3-3.5 1.4 1.3 2.3 2.2 2.3 3.5a2.3 2.3 0 0 1-2.3 2.3Z" />
    </>
  ),
  spark: (
    <>
      <path d="M9.4 4c.6 3.5 2.2 5.1 5.7 5.7-3.5.6-5.1 2.2-5.7 5.7-.6-3.5-2.2-5.1-5.7-5.7 3.5-.6 5.1-2.2 5.7-5.7Z" />
      <path d="M15.6 2.6v2.6M14.3 3.9h2.6" />
    </>
  ),
  cards: (
    <>
      <path d="M7.4 3.4h6.8a2 2 0 0 1 2 2v8.8" />
      <rect x="3.8" y="6" width="9.6" height="11.4" rx="2" />
      <path d="M8.6 8.9c.3 1.4.9 2 2.3 2.3-1.4.3-2 .9-2.3 2.3-.3-1.4-.9-2-2.3-2.3 1.4-.3 2-.9 2.3-2.3Z" />
    </>
  ),
  dumbbell: (
    <>
      <path d="M7.2 10h5.6M2.6 8.6v2.8M17.4 8.6v2.8" />
      <rect x="4.2" y="6" width="3" height="8" rx="1.2" />
      <rect x="12.8" y="6" width="3" height="8" rx="1.2" />
    </>
  ),
  wind: (
    <>
      <path d="M3 7.6h8.6a2.3 2.3 0 1 0-2.3-2.3" />
      <path d="M3 10.8h11.6a2.4 2.4 0 1 1-2.4 2.4" />
      <path d="M3 14h4.4" />
    </>
  ),
  rocket: (
    <>
      <path d="M8.3 13.2 6.8 11.7C8.6 7.1 11.5 4.3 16.6 3.4c-.9 5.1-3.7 8-8.3 9.8Z" />
      <circle cx="12.4" cy="7.6" r="1.3" />
      <path d="M7 10.8 4.2 10.5l1.9-2.4 2.9.1M9.2 13l.3 2.8 2.4-1.9-.1-2.9" />
      <path d="M5.7 14.3c-.8.6-1.1 1.9-1.1 1.9s1.3-.3 1.9-1.1" />
    </>
  ),
  trophy: (
    <>
      <path d="M6.2 3.6h7.6v4.6a3.8 3.8 0 0 1-7.6 0Z" />
      <path d="M6.2 5.2H4.5a.7.7 0 0 0-.7.8c.2 1.6 1.1 2.7 2.6 3M13.8 5.2h1.7a.7.7 0 0 1 .7.8c-.2 1.6-1.1 2.7-2.6 3" />
      <path d="M10 12v3.6M6.8 16.4h6.4" />
    </>
  ),
  rank: (
    <>
      <path d="M10 2.9 15.8 5v4.5c0 3.5-2.4 6.1-5.8 7.6-3.4-1.5-5.8-4.1-5.8-7.6V5Z" />
      <path d="m7.4 9.6 2.6-2.2 2.6 2.2M7.4 12.6l2.6-2.2 2.6 2.2" />
    </>
  ),
  reels: (
    <>
      <rect x="5" y="2.8" width="10" height="14.4" rx="2.6" />
      <path d="M8.7 7.8v4.4l3.5-2.2Z" />
    </>
  ),
  code: <path d="M6.8 6.4 3.4 10l3.4 3.6M13.2 6.4l3.4 3.6-3.4 3.6M11.3 4.8l-2.6 10.4" />,
  book: (
    <>
      <path d="M10 5.6C8.4 4.4 6.4 4 3.6 4.2v10.6c2.8-.2 4.8.2 6.4 1.4 1.6-1.2 3.6-1.6 6.4-1.4V4.2C13.6 4 11.6 4.4 10 5.6Z" />
      <path d="M10 5.6v10.6" />
    </>
  ),
  heart: <path d="M10 16.5C6.2 14 3.5 11.4 3.5 8.2A3.2 3.2 0 0 1 6.7 5c1.4 0 2.6.8 3.3 2 .7-1.2 1.9-2 3.3-2a3.2 3.2 0 0 1 3.2 3.2c0 3.2-2.7 5.8-6.5 8.3Z" />,
  restart: (
    <>
      <path d="M15.8 11a6 6 0 1 1-1.7-5.2" />
      <path d="M15.4 3.4v3.4H12" />
    </>
  ),
  meh: (
    <>
      <circle cx="10" cy="10" r="6.8" />
      <path d="M7.5 8.4h.01M12.5 8.4h.01M7.4 12.6h5.2" />
    </>
  ),
  battery: (
    <>
      <rect x="2.8" y="6.4" width="12.6" height="7.2" rx="2" />
      <path d="M17.4 8.8v2.4M5.6 9v2" />
    </>
  ),
  mountain: <path d="M2.6 15.8 7.8 6.8l3.1 5.2 1.9-2.9 4.6 6.7Z" />,
  cup: (
    <>
      <path d="M4.4 8.2h9.2v3.9a4.2 4.2 0 0 1-4.2 4.2h-.8a4.2 4.2 0 0 1-4.2-4.2Z" />
      <path d="M13.6 9.4h1.1a2 2 0 0 1 0 4h-1.3M7.6 3.6v2M10.4 3.6v2" />
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
      strokeWidth={1.5}
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
