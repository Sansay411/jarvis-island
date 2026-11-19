// Weather glyphs on the same 20px grid and stroke as the icon set. The sun is
// the only coloured part (amber, like everything alive on the island);
// clouds, rain and snow stay in the text colour.

import type { WeatherKind } from "../store/weather";

const SUN = "#ffb23f";

function Sun({ small = false }: { small?: boolean }) {
  const c = small ? { cx: 7.5, cy: 7.5, r: 2.8 } : { cx: 10, cy: 10, r: 3.6 };
  const rays = small
    ? "M7.5 2.2v1.2M7.5 11.6v1.2M2.2 7.5h1.2M11.6 7.5h1.2M3.8 3.8l.85.85M10.35 10.35l.85.85M3.8 11.2l.85-.85M10.35 4.65l.85-.85"
    : "M10 2.6v1.8M10 15.6v1.8M2.6 10h1.8M15.6 10h1.8M4.8 4.8l1.25 1.25M13.95 13.95l1.25 1.25M4.8 15.2l1.25-1.25M13.95 6.05l1.25-1.25";
  return (
    <g stroke={SUN}>
      <circle {...c} fill={SUN} fillOpacity={0.18} />
      <path d={rays} />
    </g>
  );
}

const CLOUD = "M6 15.5h8.2a3.3 3.3 0 0 0 .4-6.57A4.6 4.6 0 0 0 5.8 8.4 3.6 3.6 0 0 0 6 15.5Z";
const CLOUD_HIGH = "M6 12.5h8.2a3.3 3.3 0 0 0 .4-6.57A4.6 4.6 0 0 0 5.8 5.4 3.6 3.6 0 0 0 6 12.5Z";

export function WeatherGlyph({ kind, night = false, size = 20 }: { kind: WeatherKind; night?: boolean; size?: number }) {
  let body;
  switch (kind) {
    case "clear":
      body = night ? <path d="M14.6 12.4A6 6 0 0 1 7.6 5.4a6 6 0 1 0 7 7Z" /> : <Sun />;
      break;
    case "partly":
      body = (
        <>
          {night ? <path d="M9.4 6.3A3.6 3.6 0 0 1 5 2.6a3.9 3.9 0 1 0 4.4 3.7Z" /> : <Sun small />}
          <path d="M7.5 16.5h7.2a2.9 2.9 0 0 0 .3-5.78A4 4 0 0 0 7.3 10a3.25 3.25 0 0 0 .2 6.5Z" fill="#0a0a0c" />
        </>
      );
      break;
    case "fog":
      body = <path d="M3.5 8h13M5 11h11M3.5 14h13M6 17h8" />;
      break;
    case "rain":
      body = (
        <>
          <path d={CLOUD_HIGH} />
          <path d="M7 15l-.8 2.2M10.5 15l-.8 2.2M14 15l-.8 2.2" />
        </>
      );
      break;
    case "snow":
      body = (
        <>
          <path d={CLOUD_HIGH} />
          <path d="M7 15.6h.01M10 17h.01M13 15.6h.01M8.5 18.4h.01M11.6 18.4h.01" strokeWidth={2.2} />
        </>
      );
      break;
    case "storm":
      body = (
        <>
          <path d={CLOUD_HIGH} />
          <path d="m10.6 13.2-1.9 3h2.6l-1.6 3" stroke={SUN} />
        </>
      );
      break;
    default:
      body = <path d={CLOUD} />;
  }
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {body}
    </svg>
  );
}
