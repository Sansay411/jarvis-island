// Voice conversation, built around the face: the eyes stay in the centre of
// the brow and the sound spreads from them as two short wings — the newest
// level next to the eyes, older ones drifting outward, the mic while the user
// talks, JARVIS's own output (amber) while it speaks. Below sits the line
// being said; a running timer or stopwatch gets its own pill under it.

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../ui/Icon";
import { inputLevel, outputLevel, useVoice, type VoiceState } from "../../store/voice";
import { useAgent } from "../../store/agent";
import { useNextTimer, type Reminder } from "../../store/reminders";
import { clock, elapsedMs, useStopwatch, useTick } from "../../store/stopwatch";

const WING = 4;
const LABEL: Record<VoiceState, string> = {
  idle: "",
  dormant: "",
  connecting: "Запускаю",
  listening: "Слушаю",
  hearing: "Слышу",
  thinking: "Думаю",
  speaking: "Говорю",
  error: "Голос недоступен",
};

/** Two mirrored wings of bars on either side of the eyes. */
function Wings({ state }: { state: VoiceState }) {
  const bars = useRef<(HTMLSpanElement | null)[]>([]);
  const history = useRef<number[]>(new Array(WING).fill(0));
  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => {
    let raf = 0;
    let last = 0;
    let phase = 0;
    const frame = (t: number) => {
      raf = requestAnimationFrame(frame);
      if (t - last < 40) return; // ~25 fps is plenty for audio levels
      last = t;
      const s = stateRef.current;
      const level = s === "speaking" ? outputLevel.get() : s === "hearing" || s === "listening" ? inputLevel.get() : 0;
      const h = history.current;
      h.pop();
      h.unshift(level);
      phase += 0.35;
      for (let i = 0; i < WING * 2; i++) {
        // Left wing: index 0 is the outermost; right wing: WING is next to the eyes.
        const lag = i < WING ? WING - 1 - i : i - WING;
        const v = h[lag] ?? 0;
        const height =
          s === "thinking"
            ? 4 + 3 * (0.5 + 0.5 * Math.sin(phase - lag * 0.9)) // a slow ripple while thinking
            : 4 + v * (18 - lag * 3);
        const el = bars.current[i];
        if (el) el.style.height = `${height.toFixed(1)}px`;
      }
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  const bar = (i: number) => <span key={i} ref={(el) => void (bars.current[i] = el)} className="wing-bar" style={{ opacity: 1 - (i < WING ? WING - 1 - i : i - WING) * 0.18 }} />;
  return (
    <div className={`wings is-${state}`} aria-hidden="true">
      <div className="wing">{Array.from({ length: WING }, (_, i) => bar(i))}</div>
      <div className="wing-gap" />
      <div className="wing">{Array.from({ length: WING }, (_, i) => bar(i + WING))}</div>
    </div>
  );
}

function TimerPill({ t }: { t: Reminder }) {
  useTick(250);
  const due = Date.parse(t.dueAt);
  const total = Math.max(1000, due - Date.parse(t.createdAt));
  const left = Math.max(0, due - Date.now());
  const C = 2 * Math.PI * 6;
  return (
    <div className={`voice-clock${left <= 10_000 ? " is-urgent" : ""}`}>
      <svg className="timer-ring" viewBox="0 0 16 16" aria-hidden="true">
        <circle cx="8" cy="8" r="6" className="timer-ring-track" />
        <circle cx="8" cy="8" r="6" className="timer-ring-fill" strokeDasharray={C} strokeDashoffset={C * (1 - left / total)} />
      </svg>
      <span className="voice-clock-digits">{clock(left + 999)}</span>
      <span className="voice-clock-label">{t.text || "Таймер"}</span>
    </div>
  );
}

function StopwatchPill() {
  const s = useStopwatch();
  useTick(100, s.running);
  return (
    <div className={`voice-clock is-stopwatch${s.running ? " is-running" : ""}`}>
      <span className="clock-dot" aria-hidden="true" />
      <span className="voice-clock-digits">{clock(elapsedMs(s), true)}</span>
      <span className="voice-clock-label">{s.running ? "Секундомер" : "На паузе"}</span>
    </div>
  );
}

/** Whether the voice view shows a clock pill (the island grows for it). */
export function useVoiceClock(): boolean {
  const timer = useNextTimer();
  const sw = useStopwatch((s) => s.active);
  return !!timer || sw;
}

function VoiceClock() {
  const timer = useNextTimer();
  const sw = useStopwatch((s) => s.active);
  if (timer) return <TimerPill t={timer} />;
  return sw ? <StopwatchPill /> : null;
}

function VoiceConfirm() {
  const pending = useAgent((s) => s.items.find((it) => it.kind === "tool" && it.status === "awaiting"));
  const confirm = useAgent((s) => s.confirm);
  if (!pending || pending.kind !== "tool") return null;
  return (
    <div className={`voice-confirm risk-${pending.risk ?? "moderate"}`} role="group" aria-label="Нужно подтверждение">
      <span className="voice-confirm-title">{pending.title}</span>
      <button className="btn btn-quiet" onClick={() => void confirm(pending.id, false)}>
        Отклонить
      </button>
      <button className="btn btn-solid" onClick={() => void confirm(pending.id, true)}>
        Разрешить
      </button>
    </div>
  );
}

export function VoiceView() {
  const { state, error, userLine, assistantLine } = useVoice();
  const line = state === "error" ? error : state === "speaking" || (state === "listening" && assistantLine) ? assistantLine : userLine;
  const words = (line ?? "").trim().split(/\s+/).filter(Boolean).slice(-18);
  const mine = !(state === "speaking" || (state === "listening" && assistantLine));

  return (
    <div className="view view-voice">
      <div className="brow">
        <span className={`brow-name${state === "error" ? " is-error" : ""}`}>{LABEL[state]}</span>
        <Wings state={state} />
        <div className="brow-actions">
          {state === "error" && (
            <button className="btn btn-quiet" onClick={() => void invoke("settings_open")}>
              Настройки
            </button>
          )}
          <button className="stop-ring" aria-label="Завершить голосовой режим" onClick={() => void invoke("voice_stop")}>
            <Icon name="stop" size={10} />
          </button>
        </div>
      </div>
      <p className={`voice-line${mine ? " is-user" : ""}`} aria-live="polite">
        {words.length === 0 && state !== "error" ? (
          <span className="voice-hint">{state === "thinking" ? "Секунду…" : "Говорите — я слушаю"}</span>
        ) : (
          words.map((w, i) => (
            <span key={`${words.length}-${i}`} className={i === words.length - 1 && state !== "error" ? "word is-new" : "word"}>
              {w}{" "}
            </span>
          ))
        )}
      </p>
      <VoiceClock />
      <VoiceConfirm />
    </div>
  );
}
