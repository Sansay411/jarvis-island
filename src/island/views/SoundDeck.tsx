// «Звук»: the headphones on, every bit of their charge — each bud and the case,
// charging, in-ear, the lid, where the sound goes, how fast the charge drains
// today and how long it lasts — the volume, and the other Bluetooth devices
// that report a battery. Also the brow chip and the card that says hello when
// headphones come on or run low.

import { motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { Icon, type IconName } from "../../ui/Icon";
import { Slider } from "../../ui/Slider";
import { ipc } from "../../lib/ipc";
import type { BtDevice, BtKind, BtPart } from "../../lib/types";
import { humanMinutes, isAudio, isCharging, levelOf, levelTone, primaryAudio, useBluetooth } from "../../store/bluetooth";
import { useSystem } from "../../store/system";
import { useIsland } from "../../store/island";
import "./sound.css";

export function kindIcon(kind: BtKind): IconName {
  switch (kind) {
    case "earbuds":
      return "budLeft";
    case "headphones":
    case "headset":
    case "audio":
      return "headphones";
    case "speaker":
      return "speaker";
    case "keyboard":
      return "keyboard";
    case "mouse":
      return "mouse";
    case "gamepad":
      return "gamepad";
    case "phone":
      return "phone";
    case "watch":
      return "watch";
    case "computer":
      return "laptop";
    default:
      return "bluetooth";
  }
}

/** A charge ring with a glyph inside; dashed when the part is not reporting. */
export function BatteryRing({
  level,
  charging = false,
  icon,
  size = 58,
  stroke = 3.5,
  dim = false,
}: {
  level: number | null;
  charging?: boolean;
  icon: IconName;
  size?: number;
  stroke?: number;
  dim?: boolean;
}) {
  const reduced = useReducedMotion();
  const r = (size - stroke) / 2 - 1;
  const c = 2 * Math.PI * r;
  const frac = level == null ? 0 : Math.max(0, Math.min(1, level / 100));
  const tone = levelTone(level, charging);
  return (
    <span className={`bt-ring is-${tone}${dim ? " is-dim" : ""}`} style={{ width: size, height: size }}>
      <svg viewBox={`0 0 ${size} ${size}`} width={size} height={size} aria-hidden="true">
        <circle className={`bt-ring-track${level == null ? " is-unknown" : ""}`} cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} />
        {level != null && (
          <motion.circle
            className="bt-ring-fill"
            cx={size / 2}
            cy={size / 2}
            r={r}
            strokeWidth={stroke}
            strokeDasharray={c}
            initial={{ strokeDashoffset: c }}
            animate={{ strokeDashoffset: c * (1 - frac) }}
            transition={reduced ? { duration: 0 } : { type: "spring", stiffness: 120, damping: 22, mass: 0.9 }}
            transform={`rotate(-90 ${size / 2} ${size / 2})`}
          />
        )}
      </svg>
      <span className="bt-ring-glyph">
        <Icon name={icon} size={Math.round(size * 0.4)} />
      </span>
      {charging && (
        <span className="bt-ring-bolt" aria-label="Заряжается">
          <Icon name="bolt" size={Math.max(9, Math.round(size * 0.18))} />
        </span>
      )}
    </span>
  );
}

function pct(level: number | null): string {
  return level == null ? "—" : `${level}%`;
}

function partNote(p: BtPart | null, isCase: boolean, lidOpen: boolean | null): string {
  if (!p || p.level == null) return isCase ? "не на связи" : "нет данных";
  if (p.charging) return "заряжается";
  if (isCase) return lidOpen === true ? "открыт" : lidOpen === false ? "закрыт" : "кейс";
  if (p.inEar === true) return "в ухе";
  if (p.inEar === false) return "снят";
  return "";
}

/** Each bud and the case, AirPods-style. */
function PodsHero({ d }: { d: BtDevice }) {
  const parts: Array<{ key: string; label: string; icon: IconName; part: BtPart | null; isCase: boolean }> = [
    { key: "l", label: "Левый", icon: "budLeft", part: d.left, isCase: false },
    { key: "r", label: "Правый", icon: "budRight", part: d.right, isCase: false },
    { key: "c", label: "Кейс", icon: "budCase", part: d.case, isCase: true },
  ];
  return (
    <div className="bt-pods">
      {parts.map(({ key, label, icon, part, isCase }) => {
        const level = part?.level ?? null;
        const tone = levelTone(level, part?.charging);
        return (
          <div key={key} className={`bt-pod is-${tone}`}>
            <BatteryRing level={level} charging={part?.charging} icon={icon} dim={level == null} />
            <span className="bt-pod-level">{pct(level)}</span>
            <span className="bt-pod-label">{label}</span>
            <span className="bt-pod-note">{partNote(part, isCase, d.lidOpen)}</span>
          </div>
        );
      })}
    </div>
  );
}

/** One battery for the whole device: a big ring and the number. */
function SingleHero({ d }: { d: BtDevice }) {
  const level = levelOf(d);
  const charging = isCharging(d);
  const tone = levelTone(level, charging);
  return (
    <div className={`bt-single is-${tone}`}>
      <BatteryRing level={level} charging={charging} icon={kindIcon(d.kind)} size={92} stroke={5} dim={level == null} />
      <div className="bt-single-text">
        <span className="bt-single-level">{level == null ? "—" : level}</span>
        {level != null && <span className="bt-single-unit">%</span>}
        <span className="bt-single-caption">{level == null ? "заряд не сообщают" : charging ? "заряжаются" : "заряд"}</span>
      </div>
    </div>
  );
}

/** «за 2 ч», «за 40 мин»: how far back the line goes. */
function spanLabel(history: Array<[number, number]>): string {
  const first = history.find(([t]) => Date.now() - t <= 6 * 3600 * 1000);
  if (!first) return "";
  return `за ${humanMinutes(Math.max(1, Math.round((Date.now() - first[0]) / 60_000)))}`;
}

function ago(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 45) return "только что";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} мин назад`;
  return `${Math.round(m / 60)} ч назад`;
}

/** The level over the last hours as one quiet line, from the first sample
 *  (at most six hours back, at least one) to now. */
function Sparkline({ history, width = 244, height = 26 }: { history: Array<[number, number]>; width?: number; height?: number }) {
  const path = useMemo(() => {
    const now = Date.now();
    const pts = history.filter(([t]) => now - t <= 6 * 3600 * 1000);
    if (pts.length < 2) return null;
    const t0 = Math.min(pts[0]![0], now - 3600 * 1000);
    const x = (t: number) => ((t - t0) / (now - t0)) * width;
    const y = (l: number) => height - 2 - (l / 100) * (height - 4);
    const line = pts.map(([t, l], i) => `${i === 0 ? "M" : "L"}${x(t).toFixed(1)},${y(l).toFixed(1)}`).join(" ");
    const last = pts[pts.length - 1]!;
    const tail = `L${width},${y(last[1]).toFixed(1)}`;
    const area = `${line} ${tail} L${width},${height} L${x(pts[0]![0]).toFixed(1)},${height} Z`;
    return { line: `${line} ${tail}`, area, dot: { x: width, y: y(last[1]) } };
  }, [history, width, height]);
  if (!path) return <div className="bt-spark is-empty" style={{ height }}>График появится через несколько минут</div>;
  return (
    <svg className="bt-spark" width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
      <defs>
        <linearGradient id="bt-spark-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="currentColor" stopOpacity="0.22" />
          <stop offset="1" stopColor="currentColor" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={path.area} fill="url(#bt-spark-fill)" />
      <path d={path.line} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" strokeLinecap="round" />
      <circle cx={path.dot.x - 1.5} cy={path.dot.y} r="2.2" fill="currentColor" />
    </svg>
  );
}

function Facts({ d }: { d: BtDevice }) {
  const charging = isCharging(d);
  const level = levelOf(d);
  const tone = levelTone(level, charging);
  const status = d.connected ? (d.audioOutput ? "Звук идёт сюда" : "Подключены") : "Не подключены";
  return (
    <div className="bt-facts">
      <div className="bt-facts-head">
        <span className="bt-facts-name" title={d.name}>
          {d.model ?? d.name}
        </span>
        <span className={`bt-facts-status${d.connected ? " is-on" : ""}${d.audioOutput ? " is-output" : ""}`}>
          <span className="bt-dot" aria-hidden="true" />
          {status}
        </span>
      </div>
      {d.model && d.model !== d.name && <span className="bt-facts-sub">{d.name}</span>}
      {d.connected ? (
        <dl className="bt-stats">
          <div>
            <dt>Хватит на</dt>
            <dd>{charging ? "заряжаются" : d.minutesLeft != null ? `≈ ${humanMinutes(d.minutesLeft)}` : "считаю…"}</dd>
          </div>
          <div>
            <dt>Расход</dt>
            <dd>{d.drainPerHour != null ? `${Math.max(1, Math.round(d.drainPerHour))}% в час` : "—"}</dd>
          </div>
        </dl>
      ) : (
        <dl className="bt-stats">
          <div>
            <dt>Последний заряд</dt>
            <dd>{d.battery != null ? `${d.battery}%` : "—"}</dd>
          </div>
          <div>
            <dt>Сейчас</dt>
            <dd>не на связи</dd>
          </div>
        </dl>
      )}
      {d.connected && (
      <div className={`bt-spark-wrap is-${tone}`}>
        <Sparkline history={d.history} />
        <span className="bt-updated">
          <span>{spanLabel(d.history)}</span>
          <span>{`обновлено ${ago(d.updatedAt)}`}</span>
        </span>
      </div>
      )}
    </div>
  );
}

function Away({ devices }: { devices: BtDevice[] }) {
  const paired = devices.filter(isAudio).slice(0, 2);
  return (
    <div className="bt-away">
      <BatteryRing level={null} icon="headphones" size={72} stroke={4} dim />
      <div className="bt-away-text">
        <span className="bt-away-title">Наушники не подключены</span>
        {paired.length > 0 ? (
          paired.map((d) => (
            <span key={d.id} className="bt-away-line">
              <Icon name={kindIcon(d.kind)} size={12} />
              <span className="bt-away-name">{d.model ?? d.name}</span>
              {d.battery != null && <span className="bt-away-level">было {d.battery}%</span>}
            </span>
          ))
        ) : (
          <span className="bt-away-line is-hint">Подключите их в настройках Bluetooth — заряд появится здесь</span>
        )}
      </div>
    </div>
  );
}

function OtherDevices({ list }: { list: BtDevice[] }) {
  if (list.length === 0) return null;
  // Three chips leave no room for names: the glyph says what it is.
  const named = list.length < 3;
  return (
    <div className="bt-others">
      {list.map((d) => {
        const level = levelOf(d);
        return (
          <span key={d.id} className={`bt-chip is-${levelTone(level)}${d.connected ? "" : " is-away"}`} title={`${d.name}${d.connected ? "" : " — не на связи"}`}>
            <Icon name={kindIcon(d.kind)} size={12} />
            {named && <span className="bt-chip-name">{d.name}</span>}
            <span className="bt-chip-level">{level != null ? `${level}%` : d.connected ? "на связи" : "нет"}</span>
          </span>
        );
      })}
    </div>
  );
}

export function SoundDeck() {
  const devices = useBluetooth((s) => s.devices);
  const audio = useSystem((s) => s.audio);
  const setVolume = useSystem((s) => s.setVolume);
  const toggleMute = useSystem((s) => s.toggleMute);
  const refreshControls = useSystem((s) => s.refreshControls);
  useEffect(() => {
    void ipc.bluetooth.refresh();
    void refreshControls();
  }, [refreshControls]);
  const main = primaryAudio(devices);
  const shown = main ?? devices.find((d) => isAudio(d) && d.battery != null) ?? null;
  const others = devices.filter((d) => d.id !== main?.id && (d.connected || d.battery != null) && !(main == null && isAudio(d))).slice(0, 3);
  const pods = main && (main.left || main.right || main.case);
  return (
    <div className="sound-deck">
      <div className="sound-hero">{main ? pods ? <PodsHero d={main} /> : <SingleHero d={main} /> : <Away devices={devices} />}</div>
      <div className="nook-hairline" aria-hidden="true" />
      <div className="sound-side">
        {main ? <Facts d={main} /> : shown ? <Facts d={shown} /> : <div className="bt-facts is-empty">Здесь будет всё о заряде гарнитуры</div>}
        <div className="sound-foot">
          <Slider
            icon={audio?.muted ? "muted" : "volume"}
            label={audio ? `Громкость — ${audio.device}` : "Громкость"}
            value={audio ? (audio.muted ? 0 : audio.volume) : null}
            iconActive={audio?.muted}
            onIconClick={toggleMute}
            onChange={setVolume}
            disabled={!audio}
            disabledReason="Нет активного устройства вывода звука"
          />
          <OtherDevices list={others} />
        </div>
      </div>
    </div>
  );
}

/** The brow's headphones chip: level at a glance, a click opens «Звук». */
export function HeadsetChip() {
  const main = useBluetooth((s) => primaryAudio(s.devices));
  const setDeck = useIsland((s) => s.setDeck);
  if (!main) return null;
  const level = levelOf(main);
  const charging = isCharging(main);
  return (
    <button
      type="button"
      className={`brow-headset is-${levelTone(level, charging)}`}
      title={`${main.model ?? main.name} — заряд`}
      onClick={() => setDeck("sound")}
    >
      <Icon name={kindIcon(main.kind)} size={13} />
      {level != null && <span>{level}%</span>}
      {charging && <Icon name="bolt" size={10} />}
    </button>
  );
}

/** Headphones just came on (or ran low): who, and every part's charge. */
export function HeadsetHello() {
  const hello = useBluetooth((s) => s.hello);
  const d = useBluetooth((s) => s.devices.find((x) => x.id === s.hello?.id) ?? null);
  const dismiss = useBluetooth((s) => s.dismissHello);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!hello) return;
    const t = window.setTimeout(dismiss, hello.reason === "low" ? 7000 : 4500);
    return () => window.clearTimeout(t);
  }, [hello, dismiss]);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, []);
  if (!hello || !d) return null;
  const level = levelOf(d);
  const low = hello.reason === "low";
  const parts: Array<[IconName, BtPart | null]> = [
    ["budLeft", d.left],
    ["budRight", d.right],
    ["budCase", d.case],
  ];
  const pods = Boolean(d.left || d.right || d.case);
  return (
    <div className={`view view-hello${low ? " is-low" : ""}`} onClick={dismiss} role="status">
      <div className="hello-main">
        <BatteryRing level={level} charging={isCharging(d)} icon={kindIcon(d.kind)} size={50} stroke={3.5} />
        <div className="hello-text">
          <span className="hello-title">{d.model ?? d.name}</span>
          <span className="hello-sub">
            {low ? `Садятся — осталось ${level ?? "?"}%` : d.audioOutput ? "Подключены · звук здесь" : "Подключены"}
          </span>
        </div>
        <div className="hello-parts">
          {pods ? (
            parts.map(([icon, p]) => (
              <span key={icon} className={`hello-part is-${levelTone(p?.level ?? null, p?.charging)}`}>
                <Icon name={icon} size={13} />
                {pct(p?.level ?? null)}
              </span>
            ))
          ) : (
            <span className={`hello-part is-big is-${levelTone(level)}`}>{pct(level)}</span>
          )}
        </div>
      </div>
    </div>
  );
}
