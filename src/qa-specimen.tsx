// Specimen: a living gallery of JARVIS's design tokens and shared components.
// Served at `/qa/specimen.html`. Tokens and icon names are read from their
// source files at build time, so the page always matches the shipped code.

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { animate, useMotionValue } from "motion/react";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import "./ui/slider.css";
import "./settings/settings.css";
import "./specimen/specimen.css";
import tokensCss from "./styles/tokens.css?raw";
import iconSrc from "./ui/Icon.tsx?raw";
import { Icon, type IconName } from "./ui/Icon";
import { Slider } from "./ui/Slider";
import { WeatherGlyph } from "./ui/WeatherGlyph";
import { Group, Row, Segmented, Switch } from "./settings/controls";
import { JarvisFace } from "./island/skins/JarvisFace";
import { SKINS, type EyeMood, type SkinId } from "./island/skins/types";
import { DURATION as EMOTE_MS, type Activity, type Emote } from "./store/emote";
import type { WeatherKind } from "./store/weather";
import { groupTokens, parseIconNames, parseTokens, resolveToken, tokenKind, type Token } from "./specimen/parse";

const TOKENS = parseTokens(tokensCss);
const ICONS = parseIconNames(iconSrc) as IconName[];

const MOODS: EyeMood[] = ["idle", "attentive", "thinking", "listening", "speaking", "happy", "error", "sleep"];
const EMOTES: Emote[] = ["wink", "heart", "laugh", "surprised", "shake", "boop", "star", "sad", "determined", "squint", "confused", "dizzy", "yawn", "wake"];
const ACTIVITIES: Activity[] = ["search", "read", "type", "click", "look", "draw", "files", "music", "calendar", "memory", "time", "system", "code", "connect", "speak", "work"];
const WEATHER: WeatherKind[] = ["clear", "partly", "cloud", "fog", "rain", "snow", "storm"];

const SECTIONS = [
  { id: "colors", label: "Colour" },
  { id: "type", label: "Typography" },
  { id: "shape", label: "Radii & motion" },
  { id: "buttons", label: "Buttons" },
  { id: "controls", label: "Settings controls" },
  { id: "slider", label: "Slider" },
  { id: "icons", label: "Icons" },
  { id: "weather", label: "Weather glyphs" },
  { id: "face", label: "Face & skins" },
] as const;

/* ---------- copying ---------- */

function copyLegacy(text: string): boolean {
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  const ok = document.execCommand("copy");
  ta.remove();
  return ok;
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return copyLegacy(text);
  }
}

let toastListener: ((msg: string) => void) | null = null;
function copy(text: string, what = text) {
  void copyText(text).then((ok) =>
    toastListener?.(ok ? `Copied ${what.length > 48 ? "snippet" : what}` : "Copy blocked by the browser"),
  );
}

function Toast() {
  const [msg, setMsg] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    toastListener = (m) => {
      setMsg(m);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setMsg(null), 1600);
    };
    return () => {
      toastListener = null;
    };
  }, []);
  return (
    <div className={`sp-toast${msg ? " is-on" : ""}`} role="status" aria-live="polite">
      <Icon name="check" size={14} /> {msg}
    </div>
  );
}

function Code({ code, lang = "tsx" }: { code: string; lang?: string }) {
  const [done, setDone] = useState(false);
  return (
    <div className="sp-code">
      <div className="sp-code-bar">
        <span>{lang}</span>
        <button
          className="sp-copy"
          onClick={() => {
            copy(code);
            setDone(true);
            window.setTimeout(() => setDone(false), 1200);
          }}
        >
          <Icon name={done ? "check" : "copy"} size={13} /> {done ? "Copied" : "Copy"}
        </button>
      </div>
      <pre>
        <code>{code}</code>
      </pre>
    </div>
  );
}

/* ---------- layout ---------- */

function Section({ id, title, lead, source, children }: { id: string; title: string; lead: ReactNode; source: string; children: ReactNode }) {
  return (
    <section className="sp-section" id={id}>
      <header className="sp-section-head">
        <h2>{title}</h2>
        <p>{lead}</p>
        <span className="sp-source">
          <Icon name="file" size={12} /> {source}
        </span>
      </header>
      {children}
    </section>
  );
}

function Specimen({ title, note, children, code, dark, lang }: { title: string; note?: ReactNode; children: ReactNode; code?: string; dark?: boolean; lang?: string }) {
  return (
    <div className="sp-card">
      <div className="sp-card-head">
        <h3>{title}</h3>
        {note && <p>{note}</p>}
      </div>
      <div className={`sp-stage${dark ? " is-black" : ""}`}>{children}</div>
      {code && <Code code={code} lang={lang} />}
    </div>
  );
}

function Variant({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="sp-variant">
      <div className="sp-variant-box">{children}</div>
      <span className="sp-label">{label}</span>
    </div>
  );
}

/* ---------- colour ---------- */

function Swatch({ token }: { token: Token }) {
  const resolved = resolveToken(token.value, TOKENS);
  const alias = resolved !== token.value;
  return (
    <button className="sp-swatch" onClick={() => copy(`var(${token.name})`)} title={`Copy var(${token.name})`}>
      <span className="sp-chip" style={{ background: `var(${token.name})` }} />
      <span className="sp-swatch-name">{token.name}</span>
      <span className="sp-swatch-value">{alias ? `${token.value} → ${resolved}` : token.value}</span>
    </button>
  );
}

function Colors() {
  const groups = groupTokens(TOKENS.filter((t) => tokenKind(t, TOKENS) === "color"));
  return (
    <Section id="colors" title="Colour" lead="Every colour token from tokens.css, grouped as in the source. The island is always dark: bone for the user's own actions, amber only for JARVIS and what is live. Click a swatch to copy its var()." source="src/styles/tokens.css">
      {groups.map((g) => (
        <div key={g.group} className="sp-group">
          <h3 className="sp-group-title">{g.group}</h3>
          {g.note && <p className="sp-group-note">{g.note}</p>}
          <div className="sp-swatches">
            {g.tokens.map((t) => (
              <Swatch key={t.name} token={t} />
            ))}
          </div>
        </div>
      ))}
      <Specimen
        title="Usage"
        lang="css"
        code={`.my-card {
  background: var(--graphite-2);
  border: 1px solid var(--line);
  color: var(--text-1);
}
.my-card .hint { color: var(--text-3); }
/* Amber is JARVIS's presence: eyes, voice, live states only. */
.my-card.is-live { box-shadow: 0 0 0 1px var(--amber); }`}
      >
        <div className="sp-demo-card">
          <div className="sp-demo-title">Идёт фокус</div>
          <div className="sp-demo-hint">ещё 25 мин · до 16:30</div>
        </div>
        <div className="sp-demo-card is-live">
          <div className="sp-demo-title">Говорю</div>
          <div className="sp-demo-hint">is-live → amber rim</div>
        </div>
      </Specimen>
    </Section>
  );
}

/* ---------- typography ---------- */

const PANGRAM = "Съешь же ещё этих мягких французских булок, да выпей чаю";

function Typography() {
  const sizes = TOKENS.filter((t) => t.name.startsWith("--fs-"));
  const fonts = TOKENS.filter((t) => t.name.startsWith("--font-"));
  return (
    <Section id="type" title="Typography" lead="One UI face, Onest, with tabular figures so clocks and counters never jitter. JetBrains Mono is for code only." source="src/styles/tokens.css · docs/DESIGN.md">
      <div className="sp-grid-2">
        {fonts.map((f) => (
          <Specimen key={f.name} title={f.name} note={f.value} code={`font-family: var(${f.name});`} lang="css">
            <div className="sp-font" style={{ fontFamily: `var(${f.name})` }}>
              <div className="sp-font-aa">Аа Bb 0123</div>
              <div className="sp-font-line">{PANGRAM}</div>
              <div className="sp-font-line is-dim">The quick brown fox jumps over the lazy dog</div>
            </div>
          </Specimen>
        ))}
      </div>
      <Specimen title="Size scale" note="Click a row to copy the token.">
        <div className="sp-scale">
          {sizes.map((s) => (
            <button key={s.name} className="sp-scale-row" onClick={() => copy(`var(${s.name})`)}>
              <span className="sp-scale-meta">
                {s.name} <em>{s.value}</em>
              </span>
              <span style={{ fontSize: `var(${s.name})` }}>Джарвис, поставь фокус на 25 минут</span>
            </button>
          ))}
        </div>
      </Specimen>
      <div className="sp-grid-2">
        <Specimen title="Weights" note="Onest is variable; the app uses these stops." code={`font-weight: 560; /* .btn */\nfont-weight: 650; /* captions */`} lang="css">
          <div className="sp-weights">
            {[400, 500, 560, 600, 650, 700].map((w) => (
              <div key={w} style={{ fontWeight: w }}>
                <span className="sp-label">{w}</span> Обзор · Фокус · Рост
              </div>
            ))}
          </div>
        </Specimen>
        <Specimen
          title="Caption"
          note="One style for every label above content."
          lang="css"
          code={`.caption {
  font-size: 10px;
  font-weight: 650;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-3);
}`}
        >
          <div className="sp-captions">
            <span className="sp-caption">Идёт фокус</span>
            <span className="sp-caption">Сейчас</span>
            <span className="sp-caption">Говорю</span>
          </div>
        </Specimen>
      </div>
      <Specimen title="Tabular figures" note="tnum keeps time and counters from shifting width." code={`font-variant-numeric: tabular-nums;`} lang="css">
        <div className="sp-tnum">
          <div>
            <span className="sp-label">proportional</span>
            <span>11:11 · 10:08 · 41:17</span>
          </div>
          <div>
            <span className="sp-label">tabular</span>
            <span style={{ fontVariantNumeric: "tabular-nums" }}>11:11 · 10:08 · 41:17</span>
          </div>
        </div>
      </Specimen>
    </Section>
  );
}

/* ---------- radii & motion ---------- */

function Shape() {
  const radii = TOKENS.filter((t) => t.name.startsWith("--r-"));
  const [on, setOn] = useState(false);
  const easing = TOKENS.find((t) => t.name === "--ease-out")!;
  const durs = TOKENS.filter((t) => t.name.startsWith("--dur-"));
  return (
    <Section id="shape" title="Radii & motion" lead="The island owns the large radii; inner elements step down. Motion answers an action or a state change; nothing animates at rest." source="src/styles/tokens.css">
      <Specimen title="Radii" note="Click to copy." code={`border-radius: var(--r-md);`} lang="css">
        <div className="sp-radii">
          {radii.map((r) => (
            <button key={r.name} className="sp-radius" onClick={() => copy(`var(${r.name})`)}>
              <span className="sp-radius-box" style={{ borderRadius: `var(${r.name})` }} />
              <span className="sp-swatch-name">{r.name}</span>
              <span className="sp-swatch-value">{r.value}</span>
            </button>
          ))}
        </div>
      </Specimen>
      <Specimen
        title="Easing & duration"
        note={`${easing.name}: ${easing.value}`}
        lang="css"
        code={`transition: background var(--dur-fast) var(--ease-out),
  transform var(--dur-med) var(--ease-out);`}
      >
        <div className="sp-motion">
          {durs.map((d) => (
            <div key={d.name} className="sp-motion-row">
              <span className="sp-scale-meta">
                {d.name} <em>{d.value}</em>
              </span>
              <div className="sp-motion-track">
                <span className="sp-motion-dot" style={{ left: on ? "calc(100% - 36px)" : "4px", transition: `left var(${d.name}) var(--ease-out)` }} />
              </div>
            </div>
          ))}
          <button className="btn btn-quiet" onClick={() => setOn((v) => !v)}>
            Play
          </button>
        </div>
      </Specimen>
    </Section>
  );
}

/* ---------- buttons ---------- */

function Buttons() {
  return (
    <Section id="buttons" title="Buttons" lead="Shared by the island and settings. Solid (bone) is the user's primary action; quiet is everything else." source="src/styles/base.css">
      <Specimen
        title=".btn"
        code={`<button className="btn btn-solid">На полку</button>
<button className="btn btn-quiet">Позже</button>
<button className="btn btn-solid" disabled>Недоступно</button>`}
      >
        <Variant label="btn-solid">
          <button className="btn btn-solid">На полку</button>
        </Variant>
        <Variant label="btn-quiet">
          <button className="btn btn-quiet">Позже</button>
        </Variant>
        <Variant label="with icon">
          <button className="btn btn-solid sp-btn-icon">
            <Icon name="play" size={14} /> Фокус 1 ч 30 мин
          </button>
        </Variant>
        <Variant label="disabled">
          <button className="btn btn-solid" disabled style={{ opacity: 0.4 }}>
            Недоступно
          </button>
        </Variant>
        <Variant label="quiet · disabled">
          <button className="btn btn-quiet" disabled style={{ opacity: 0.4 }}>
            Недоступно
          </button>
        </Variant>
      </Specimen>
    </Section>
  );
}

/* ---------- settings controls ---------- */

function Controls() {
  const [a, setA] = useState(true);
  const [b, setB] = useState(false);
  const [seg, setSeg] = useState<"normal" | "strong" | "off">("normal");
  return (
    <Section id="controls" title="Settings controls" lead="Label left, control right, hairlines between rows: not a pile of cards." source="src/settings/controls.tsx">
      <div className="sp-grid-2">
        <Specimen
          title="Switch"
          code={`import { Switch } from "./settings/controls";

const [on, setOn] = useState(true);
<Switch checked={on} onChange={setOn} label="Слушать «Джарвис»" />`}
        >
          <Variant label={a ? "checked" : "unchecked"}>
            <Switch checked={a} onChange={setA} label="Демо 1" />
          </Variant>
          <Variant label={b ? "checked" : "unchecked"}>
            <Switch checked={b} onChange={setB} label="Демо 2" />
          </Variant>
        </Specimen>
        <Specimen
          title="Segmented"
          note="The thumb slides between options (motion layoutId)."
          code={`import { Segmented } from "./settings/controls";

<Segmented
  label="Шумоподавление"
  value={mode}
  onChange={setMode}
  options={[
    { id: "normal", label: "Обычное" },
    { id: "strong", label: "Сильное" },
    { id: "off", label: "Выкл" },
  ]}
/>`}
        >
          <Segmented
            label="sp-noise"
            value={seg}
            onChange={setSeg}
            options={[
              { id: "normal", label: "Обычное" },
              { id: "strong", label: "Сильное" },
              { id: "off", label: "Выкл" },
            ]}
          />
        </Specimen>
      </div>
      <Specimen
        title="Group + Row"
        note="Row takes a label, an optional hint and the control; wide puts a fixed 180 px label column."
        code={`import { Group, Row, Switch } from "./settings/controls";

<Group title="Голос">
  <Row label="Слушать «Джарвис»" hint="Микрофон обрабатывается на устройстве.">
    <Switch checked={wake} onChange={setWake} label="Слушать «Джарвис»" />
  </Row>
  <Row label="Шумоподавление">
    <Segmented label="noise" value={mode} onChange={setMode} options={MODES} />
  </Row>
</Group>`}
      >
        <div className="sp-rows">
          <Group title="Голос">
            <Row label="Слушать «Джарвис»" hint="Микрофон обрабатывается на устройстве, в сеть уходит только распознанная фраза.">
              <Switch checked={a} onChange={setA} label="Слушать" />
            </Row>
            <Row label="Шумоподавление" hint="«Сильное» для шумных комнат.">
              <Segmented
                label="sp-noise-2"
                value={seg}
                onChange={setSeg}
                options={[
                  { id: "normal", label: "Обычное" },
                  { id: "strong", label: "Сильное" },
                  { id: "off", label: "Выкл" },
                ]}
              />
            </Row>
            <Row label="Горячая клавиша" wide>
              <span className="keycaps">
                <kbd>Ctrl</kbd>
                <kbd>Alt</kbd>
                <kbd>Space</kbd>
              </span>
            </Row>
          </Group>
        </div>
      </Specimen>
    </Section>
  );
}

/* ---------- slider ---------- */

function Sliders() {
  const [vol, setVol] = useState(62);
  const [muted, setMuted] = useState(false);
  const [bri, setBri] = useState(40);
  return (
    <Section id="slider" title="Slider" lead="Pointer and keyboard (arrows, Shift for ×5), throttled onChange, optimistic value until the verified one arrives." source="src/ui/Slider.tsx">
      <Specimen
        title="Slider"
        code={`import { Slider } from "./ui/Slider";

<Slider
  icon={muted ? "muted" : "volume"}
  label="Громкость"
  value={volume}            // null shows «—»
  onChange={setVolume}
  onIconClick={toggleMute}
  iconActive={muted}        // red icon
/>
<Slider icon="sun" label="Яркость" value={null} onChange={() => {}}
  disabled disabledReason="Этот монитор не поддерживает DDC/CI" />`}
      >
        <div className="sp-sliders">
          <Variant label="default · icon toggles mute">
            <Slider icon={muted ? "muted" : "volume"} label="Громкость" value={vol} onChange={setVol} onIconClick={() => setMuted((m) => !m)} iconActive={muted} />
          </Variant>
          <Variant label="no icon action">
            <Slider icon="sun" label="Яркость" value={bri} onChange={setBri} />
          </Variant>
          <Variant label="unknown value">
            <Slider icon="mic" label="Микрофон" value={null} onChange={() => {}} />
          </Variant>
          <Variant label="disabled">
            <Slider icon="sun" label="Яркость" value={null} onChange={() => {}} disabled disabledReason="Этот монитор не поддерживает DDC/CI" />
          </Variant>
        </div>
      </Specimen>
    </Section>
  );
}

/* ---------- icons ---------- */

function Icons() {
  const [q, setQ] = useState("");
  const [size, setSize] = useState(20);
  const [picked, setPicked] = useState<IconName>("play");
  const shown = useMemo(() => ICONS.filter((n) => n.toLowerCase().includes(q.trim().toLowerCase())), [q]);
  const snippet = `import { Icon } from "./ui/Icon";

<Icon name="${picked}"${size !== 18 ? ` size={${size}}` : ""} />`;
  return (
    <Section id="icons" title="Icons" lead={`${ICONS.length} line glyphs on a 20 px grid with rounded joins, drawn to share one optical weight with the eyes. They inherit currentColor.`} source="src/ui/Icon.tsx">
      <div className="sp-toolbar">
        <label className="sp-search">
          <Icon name="search" size={14} />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter icons…" />
        </label>
        <Segmented
          label="sp-icon-size"
          value={String(size) as "16" | "18" | "20" | "24" | "32"}
          onChange={(v) => setSize(Number(v))}
          options={[16, 18, 20, 24, 32].map((s) => ({ id: String(s) as "16", label: `${s}` }))}
        />
      </div>
      <div className="sp-icons">
        {shown.map((n) => (
          <button
            key={n}
            className={`sp-icon${picked === n ? " is-on" : ""}`}
            onClick={() => {
              setPicked(n);
              copy(`<Icon name="${n}"${size !== 18 ? ` size={${size}}` : ""} />`, `<Icon name="${n}" />`);
            }}
            title={`Copy <Icon name="${n}" />`}
          >
            <Icon name={n} size={size} />
            <span>{n}</span>
          </button>
        ))}
        {shown.length === 0 && <p className="sp-empty">No icon matches «{q}».</p>}
      </div>
      <Code code={snippet} />
    </Section>
  );
}

/* ---------- weather ---------- */

function Weather() {
  return (
    <Section id="weather" title="Weather glyphs" lead="Same grid and stroke as the icons. The sun (and the storm's bolt) is the only colour, amber like everything alive on the island." source="src/ui/WeatherGlyph.tsx">
      <Specimen
        title="WeatherGlyph"
        note="kind × night"
        code={`import { WeatherGlyph } from "./ui/WeatherGlyph";

<WeatherGlyph kind="partly" night={isNight} size={28} />`}
      >
        <div className="sp-weather">
          {WEATHER.map((k) => (
            <button key={k} className="sp-weather-cell" onClick={() => copy(`<WeatherGlyph kind="${k}" />`)}>
              <div className="sp-weather-pair">
                <WeatherGlyph kind={k} size={32} />
                <WeatherGlyph kind={k} night size={32} />
              </div>
              <span className="sp-label">{k}</span>
            </button>
          ))}
        </div>
      </Specimen>
    </Section>
  );
}

/* ---------- face ---------- */

function useDemoLevel(active: boolean) {
  const level = useMotionValue(0);
  useEffect(() => {
    if (!active) {
      level.set(0);
      return;
    }
    let t = 0;
    const id = window.setInterval(() => {
      t += 1;
      const v = 0.25 + 0.55 * Math.abs(Math.sin(t * 0.9)) * (0.6 + 0.4 * Math.random());
      animate(level, v, { duration: 0.12 });
    }, 140);
    return () => window.clearInterval(id);
  }, [active, level]);
  return level;
}

function FaceCell({ skin, mood, label, scale = 2.2, activity = null, emote = null, emoteSeq = 0, onClick, active }: { skin: SkinId; mood: EyeMood; label: string; scale?: number; activity?: Activity | null; emote?: Emote | null; emoteSeq?: number; onClick?: () => void; active?: boolean }) {
  const level = useDemoLevel(mood === "listening" || mood === "speaking");
  return (
    <button className={`sp-face${active ? " is-on" : ""}`} onClick={onClick}>
      <div className="sp-face-screen">
        <JarvisFace skin={skin} mood={mood} level={level} scale={scale} activity={activity} emote={emote} emoteSeq={emoteSeq} />
      </div>
      <span className="sp-label">{label}</span>
    </button>
  );
}

function Face() {
  const [skin, setSkin] = useState<SkinId>("amber");
  const [mood, setMood] = useState<EyeMood>("attentive");
  const [emote, setEmote] = useState<Emote | null>(null);
  const [seq, setSeq] = useState(0);
  // Like the island, a one-shot emote clears itself once it has played.
  useEffect(() => {
    if (!emote) return;
    const id = window.setTimeout(() => setEmote(null), EMOTE_MS[emote]);
    return () => window.clearTimeout(id);
  }, [emote, seq]);
  const [activity, setActivity] = useState<Activity | null>(null);
  const level = useDemoLevel(mood === "listening" || mood === "speaking");
  const props = [`skin="${skin}"`, `mood="${mood}"`, ...(mood === "listening" || mood === "speaking" ? ["level={audioLevel}"] : []), ...(activity ? [`activity="${activity}"`] : []), ...(emote ? [`emote="${emote}"`, "emoteSeq={seq}"] : [])];
  const snippet = `import { JarvisFace } from "./island/skins/JarvisFace";

// level: MotionValue<number> 0..1, e.g. useMotionValue(0)
<JarvisFace
  ${props.join("\n  ")}
/>`;
  return (
    <Section id="face" title="Face & skins" lead="The island is JARVIS's face. JarvisFace renders the chosen skin (or the saved one from usePrefs) with moods, one-shot emotes and a micro-glyph for the running tool." source="src/island/skins/*">
      <Specimen title="Skins" note="Each skin also exports its presence accent through skinCssVars(skin).">
        <div className="sp-skins">
          {SKINS.map((s) => (
            <button key={s.id} className={`sp-skin${skin === s.id ? " is-on" : ""}`} onClick={() => setSkin(s.id)} style={{ ["--acc" as string]: s.accent }}>
              <div className="sp-face-screen">
                <JarvisFace skin={s.id} mood="attentive" scale={2} />
              </div>
              <div className="sp-skin-meta">
                <strong>{s.label}</strong>
                <span>{s.desc}</span>
                <span className="sp-skin-dots">
                  {[s.accentLo, s.accent, s.accentHi].map((c) => (
                    <i key={c} style={{ background: c }} title={c} />
                  ))}
                  <code>{s.id}</code>
                </span>
              </div>
            </button>
          ))}
        </div>
      </Specimen>

      <div className="sp-playground">
        <div className="sp-playground-stage">
          <JarvisFace skin={skin} mood={mood} level={level} scale={5} activity={activity} emote={emote} emoteSeq={seq} />
        </div>
        <div className="sp-playground-controls">
          <div>
            <span className="sp-caption">Mood</span>
            <div className="sp-pills">
              {MOODS.map((m) => (
                <button key={m} className={`sp-pill${mood === m ? " is-on" : ""}`} onClick={() => setMood(m)}>
                  {m}
                </button>
              ))}
            </div>
          </div>
          <div>
            <span className="sp-caption">Emote (one-shot)</span>
            <div className="sp-pills">
              <button className={`sp-pill${emote === null ? " is-on" : ""}`} onClick={() => setEmote(null)}>
                none
              </button>
              {EMOTES.map((e) => (
                <button
                  key={e}
                  className={`sp-pill${emote === e ? " is-on" : ""}`}
                  onClick={() => {
                    setEmote(e);
                    setSeq((n) => n + 1);
                  }}
                >
                  {e}
                </button>
              ))}
            </div>
          </div>
          <div>
            <span className="sp-caption">Activity (tool running)</span>
            <div className="sp-pills">
              <button className={`sp-pill${activity === null ? " is-on" : ""}`} onClick={() => setActivity(null)}>
                none
              </button>
              {ACTIVITIES.map((a) => (
                <button key={a} className={`sp-pill${activity === a ? " is-on" : ""}`} onClick={() => setActivity(a)}>
                  {a}
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>
      <Code code={snippet} />

      <Specimen title={`Moods · ${SKINS.find((s) => s.id === skin)!.label}`} note="Click one to load it into the playground." dark>
        <div className="sp-faces">
          {MOODS.map((m) => (
            <FaceCell key={m} skin={skin} mood={m} label={m} active={mood === m} onClick={() => setMood(m)} />
          ))}
        </div>
      </Specimen>
    </Section>
  );
}

/* ---------- page ---------- */

function useActiveSection() {
  const [active, setActive] = useState<string>(SECTIONS[0].id);
  useEffect(() => {
    const els = SECTIONS.map((s) => document.getElementById(s.id)).filter(Boolean) as HTMLElement[];
    const io = new IntersectionObserver(
      (entries) => {
        const vis = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (vis[0]) setActive(vis[0].target.id);
      },
      { rootMargin: "-10% 0px -70% 0px" },
    );
    els.forEach((el) => io.observe(el));
    return () => io.disconnect();
  }, []);
  return active;
}

function App() {
  const active = useActiveSection();
  const colorCount = TOKENS.filter((t) => tokenKind(t, TOKENS) === "color").length;
  return (
    <div className="sp">
      <nav className="sp-rail">
        <div className="sp-brand">
          <div className="sp-brand-face">
            <JarvisFace skin="amber" mood="attentive" scale={1} />
          </div>
          <div>
            <strong>Specimen</strong>
            <span>JARVIS design system</span>
          </div>
        </div>
        {SECTIONS.map((s) => (
          <a key={s.id} href={`#${s.id}`} className={active === s.id ? "is-on" : ""}>
            {s.label}
          </a>
        ))}
        <div className="sp-rail-foot">
          {TOKENS.length} tokens · {ICONS.length} icons · {SKINS.length} skins
        </div>
      </nav>
      <main className="sp-main">
        <header className="sp-hero">
          <span className="sp-caption">Design system · v0.8</span>
          <h1>JARVIS Specimen</h1>
          <p>
            A living gallery of the tokens and shared components in this repository. Everything here renders the real code: tokens are parsed from <code>tokens.css</code>, icon names from <code>Icon.tsx</code>. Click any swatch, size, radius or icon to copy it.
          </p>
          <div className="sp-stats">
            <div>
              <strong>{colorCount}</strong>
              <span>colour tokens</span>
            </div>
            <div>
              <strong>{ICONS.length}</strong>
              <span>icons</span>
            </div>
            <div>
              <strong>{SKINS.length}</strong>
              <span>face skins</span>
            </div>
            <div>
              <strong>{MOODS.length}</strong>
              <span>moods</span>
            </div>
          </div>
        </header>
        <Colors />
        <Typography />
        <Shape />
        <Buttons />
        <Controls />
        <Sliders />
        <Icons />
        <Weather />
        <Face />
      </main>
      <Toast />
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
