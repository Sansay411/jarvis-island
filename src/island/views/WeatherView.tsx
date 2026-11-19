// The weather card: opens by itself for a few seconds when JARVIS looks the
// weather up — on the day the answer is about (`focus`: today or tomorrow).
// Left: the big number and the sky. Right: the temperature as a curve over
// the next hours (or tomorrow 06…21) with the sky above each point. Bottom:
// a quiet line of facts. Click closes it.

import { deg, useWeather, type Weather, type WeatherHour } from "../../store/weather";
import { WeatherGlyph } from "../../ui/WeatherGlyph";

export function WeatherView() {
  const w = useWeather((s) => s.card);
  const dismiss = useWeather((s) => s.dismiss);
  if (!w) return null;
  const tomorrow = w.focus === "tomorrow" && w.tomorrow ? w.tomorrow : null;
  const city = w.place.city || w.place.country;

  return (
    <div
      className={`view view-weather is-${tomorrow ? tomorrow.kind : w.kind}${!tomorrow && !w.isDay ? " is-night" : ""}`}
      onClick={dismiss}
      role="region"
      aria-label={`Погода${tomorrow ? " на завтра" : ""}: ${city}`}
    >
      <div className="wx-glow" aria-hidden="true" />
      <div className="brow brow-split">
        <span className="wx-place" title={w.place.approximate ? "Город определён по сети" : undefined}>
          {tomorrow ? <span className="wx-day">Завтра</span> : <span className="wx-day">Сейчас</span>}
          {city}
        </span>
        <div className="brow-eyes-slot" aria-hidden="true" />
        <RangeBar min={(tomorrow ?? w.today).min} max={(tomorrow ?? w.today).max} now={tomorrow ? null : w.temp} />
      </div>

      <div className="wx-body">
        {tomorrow ? (
          <div className="wx-now">
            <span className={`wx-now-glyph is-${tomorrow.kind}`}>
              <WeatherGlyph kind={tomorrow.kind} size={40} />
            </span>
            <div className="wx-now-text">
              <span className="wx-temp">
                {deg(tomorrow.max)}
                <span className="wx-temp-low">{deg(tomorrow.min)}</span>
              </span>
              <span className="wx-sky">{tomorrow.text}</span>
            </div>
          </div>
        ) : (
          <div className="wx-now">
            <span className={`wx-now-glyph is-${w.kind}`}>
              <WeatherGlyph kind={w.kind} night={!w.isDay} size={40} />
            </span>
            <div className="wx-now-text">
              <span className="wx-temp">{deg(w.temp)}</span>
              <span className="wx-sky">{w.text}</span>
            </div>
          </div>
        )}
        <HourCurve hours={tomorrow ? w.tomorrowHours ?? [] : w.hours} />
      </div>

      <div className="wx-facts">{facts(w, !!tomorrow).map((f) => <span key={f}>{f}</span>)}</div>
    </div>
  );
}

function facts(w: Weather, tomorrow: boolean): string[] {
  if (tomorrow && w.tomorrow) {
    const t = w.tomorrow;
    const out = [`ветер до ${Math.round(t.wind ?? 0)} м/с`];
    out.push(t.precip >= 20 ? `осадки ${t.precip}%` : "без осадков");
    if (t.sunrise && t.sunset) out.push(`солнце ${t.sunrise}–${t.sunset}`);
    return out;
  }
  const out = [`ощущается ${deg(w.feels)}`, `ветер ${Math.round(w.wind)} м/с`, `влажность ${w.humidity}%`];
  if (w.tomorrow) out.push(`завтра ${deg(w.tomorrow.min)}…${deg(w.tomorrow.max)}`);
  return out;
}

/** Today's (or tomorrow's) span, with where it is now on it. */
function RangeBar({ min, max, now }: { min: number; max: number; now: number | null }) {
  const span = Math.max(1, max - min);
  const at = now === null ? null : Math.min(1, Math.max(0, (now - min) / span));
  return (
    <span className="wx-range" aria-label={`от ${deg(min)} до ${deg(max)}`}>
      <span className="wx-range-num">{deg(min)}</span>
      <span className="wx-range-bar">
        {at !== null && <span className="wx-range-dot" style={{ left: `${at * 100}%` }} />}
      </span>
      <span className="wx-range-num">{deg(max)}</span>
    </span>
  );
}

const CHART_TOP = 20; // room for the temperature over the highest point
const CHART_H = 26;

/** Sky glyphs over a smooth temperature line, the hour under each point. */
function HourCurve({ hours }: { hours: WeatherHour[] }) {
  if (hours.length === 0) return <div className="wx-hours" />;
  const temps = hours.map((h) => h.temp);
  const lo = Math.min(...temps);
  const hi = Math.max(...temps);
  const span = Math.max(3, hi - lo);
  const n = hours.length;
  const x = (i: number) => ((i + 0.5) / n) * 100;
  const y = (t: number) => CHART_TOP + (1 - (t - lo) / span) * CHART_H;
  const pts: Array<[number, number]> = hours.map((h, i) => [x(i), y(h.temp)]);
  const at = (i: number): [number, number] => pts[Math.min(n - 1, Math.max(0, i))] ?? [0, 0];
  // Catmull-Rom → cubic Bézier, so the line breathes instead of zig-zagging.
  let d = `M ${at(0)[0]} ${at(0)[1]}`;
  for (let i = 0; i < n - 1; i++) {
    const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)];
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += ` C ${c1[0]} ${c1[1]}, ${c2[0]} ${c2[1]}, ${p2[0]} ${p2[1]}`;
  }
  const area = `${d} L ${at(n - 1)[0]} ${CHART_TOP + CHART_H + 8} L ${at(0)[0]} ${CHART_TOP + CHART_H + 8} Z`;
  const h = CHART_TOP + CHART_H + 8;

  return (
    <div className="wx-hours" style={{ ["--n" as string]: n }}>
      <div className="wx-hour-glyphs">
        {hours.map((p, i) => {
          const hh = Number(p.time.slice(0, 2));
          return (
            <span key={p.time} className={`wx-hour-glyph is-${p.kind}`} style={{ animationDelay: `${60 + i * 45}ms` }}>
              <WeatherGlyph kind={p.kind} night={hh >= 19 || hh < 7} size={18} />
              {p.precip >= 30 && <span className="wx-hour-precip">{p.precip}%</span>}
            </span>
          );
        })}
      </div>
      <div className="wx-chart" style={{ height: h }}>
        <svg viewBox={`0 0 100 ${h}`} preserveAspectRatio="none" aria-hidden="true">
          <defs>
            <linearGradient id="wx-fill" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="currentColor" stopOpacity="0.16" />
              <stop offset="1" stopColor="currentColor" stopOpacity="0" />
            </linearGradient>
          </defs>
          <path d={area} fill="url(#wx-fill)" />
          <path d={d} fill="none" stroke="currentColor" strokeWidth="1.5" vectorEffect="non-scaling-stroke" strokeLinecap="round" />
        </svg>
        {hours.map((p, i) => (
          <span key={p.time} className="wx-point" style={{ left: `${x(i)}%`, top: y(p.temp) }}>
            <span className="wx-point-temp">{deg(p.temp)}</span>
          </span>
        ))}
      </div>
      <div className="wx-hour-times">
        {hours.map((p) => (
          <span key={p.time}>{p.time}</span>
        ))}
      </div>
    </div>
  );
}
