// Voice conversation: JARVIS's eyes become the two central bars of a live
// visualiser. Every bar height comes from real audio levels — the mic while
// the user talks, JARVIS's own output while it speaks. Older samples drift
// outward, so the shape is a short, mirrored history of the actual signal.

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../ui/Icon";
import { inputLevel, outputLevel, useVoice, type VoiceState } from "../../store/voice";
import { useAgent } from "../../store/agent";

const BARS = 8;
const LABEL: Record<VoiceState, string> = {
  idle: "",
  connecting: "Запускаю",
  listening: "Слушаю",
  hearing: "Слышу вас",
  thinking: "Думаю",
  speaking: "Говорю",
  error: "Голос недоступен",
};

function Visualizer({ state }: { state: VoiceState }) {
  const bars = useRef<(HTMLSpanElement | null)[]>([]);
  const history = useRef<number[]>(new Array(BARS / 2).fill(0));
  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => {
    let raf = 0;
    let last = 0;
    const frame = (t: number) => {
      raf = requestAnimationFrame(frame);
      if (t - last < 33) return; // ~30 fps is plenty for audio levels
      last = t;
      const s = stateRef.current;
      const level = s === "speaking" ? outputLevel.get() : s === "hearing" || s === "listening" ? inputLevel.get() : 0;
      const h = history.current;
      h.pop();
      h.unshift(level);
      for (let i = 0; i < BARS; i++) {
        // mirrored: centre bars show the newest sample
        const lag = i < BARS / 2 ? BARS / 2 - 1 - i : i - BARS / 2;
        const v = h[lag] ?? 0;
        const centre = lag === 0;
        const min = centre ? 12 : 6;
        const max = centre ? 46 : 40 - lag * 6;
        const height = s === "thinking" ? (centre ? 10 : 6) : min + v * (max - min);
        const el = bars.current[i];
        if (el) el.style.height = `${height.toFixed(1)}px`;
      }
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div className={`visualizer is-${state}`} aria-hidden="true">
      {Array.from({ length: BARS }, (_, i) => (
        <span key={i} ref={(el) => void (bars.current[i] = el)} className={i === BARS / 2 - 1 || i === BARS / 2 ? "bar eye" : "bar"} />
      ))}
    </div>
  );
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
  const words = (line ?? "").trim().split(/\s+/).filter(Boolean).slice(-14);

  return (
    <div className="view view-voice">
      <div className="brow">
        <span className={`brow-name${state === "error" ? " is-error" : ""}`}>{LABEL[state]}</span>
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
      <Visualizer state={state} />
      <VoiceConfirm />
      <p className="voice-line" aria-live="polite">
        {words.map((w, i) => (
          <span key={`${words.length}-${i}`} className={i === words.length - 1 && state !== "error" ? "word is-new" : "word"}>
            {w}{" "}
          </span>
        ))}
      </p>
    </div>
  );
}
