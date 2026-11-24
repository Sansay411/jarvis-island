// Settings → Голос → «Микрофон»: sensitivity, noise suppression, a live
// meter of what JARVIS hears after processing (with the room's noise floor)
// and the guided «Проверить микрофон» test.

import { AnimatePresence, motion, useMotionValue, useReducedMotion, useSpring, useTransform } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { hasTauri } from "../lib/workspace-ipc";
import { Group, Row, Segmented } from "./controls";
import {
  clampGain,
  formatDb,
  formatGain,
  meterFraction,
  phasePrompt,
  snrWord,
  testProgress,
  type MicGainMode,
  type MicMeter,
  type MicTestPhase,
  type MicTestResult,
  type NoiseMode,
} from "./micLevels";

export interface MicSettings {
  inputDevice: string;
  micGainMode: MicGainMode;
  micGainDb: number;
  noiseSuppression: NoiseMode;
}

const PING_MS = 2000;

/** The browser preview's test result (QA: `?mic=result` shows it at once). */
const DEMO_RESULT: MicTestResult = {
  deviceName: "Микрофон (демо)",
  noiseFloorDb: -63,
  postFloorDb: -84,
  speechLevelDb: -41,
  postSpeechDb: -21,
  snrDb: 22,
  appliedGainDb: 21,
  recommendedGainDb: 21,
  transcript: "Джарвис, какая сегодня погода?",
  wakeDetected: true,
  sttAvailable: true,
  advice: "Микрофон тихий — включил усиление +21 дБ.",
  severity: "ok",
};

/** The params the backend's meter and test take (`MicParams`). */
function micParams(s: MicSettings) {
  return { device: s.inputDevice, gainMode: s.micGainMode, gainDb: clampGain(s.micGainDb), noise: s.noiseSuppression };
}

/** A believable meter for the browser preview (no Tauri). */
function useDemoMeter(on: boolean, set: (m: MicMeter) => void) {
  useEffect(() => {
    if (!on) return;
    let t = 0;
    const id = window.setInterval(() => {
      t += 0.05;
      const word = Math.max(0, Math.sin(t * 2.1)) * Math.abs(Math.sin(t * 9));
      set({ db: -62 + word * 42, floorDb: -61, gainDb: 14, speech: word > 0.3, device: "Микрофон (демо)", error: null });
    }, 50);
    return () => window.clearInterval(id);
  }, [on, set]);
}

export function MicCheck({ value, onChange }: { value: MicSettings; onChange: (patch: Partial<MicSettings>) => void }) {
  const tauri = hasTauri();
  const reduced = useReducedMotion();
  const [meter, setMeter] = useState<MicMeter | null>(null);
  const [phase, setPhase] = useState<MicTestPhase>("idle");
  const [progress, setProgress] = useState(0);
  const [result, setResult] = useState<MicTestResult | null>(() =>
    !tauri && new URLSearchParams(window.location.search).get("mic") === "result" ? DEMO_RESULT : null,
  );
  const [error, setError] = useState<string | null>(null);
  const params = useMemo(() => micParams(value), [value]);
  const paramsRef = useRef(params);
  paramsRef.current = params;

  // Keep the backend meter alive while this section is on screen.
  useEffect(() => {
    if (!tauri) return;
    const ping = () => {
      if (document.visibilityState === "visible") {
        void invoke("voice_mic_monitor", { on: true, mic: paramsRef.current }).catch(() => {});
      }
    };
    ping();
    const id = window.setInterval(ping, PING_MS);
    return () => {
      window.clearInterval(id);
      void invoke("voice_mic_monitor", { on: false, mic: paramsRef.current }).catch(() => {});
    };
  }, [tauri]);

  // New settings reach the meter at once, not on the next ping.
  useEffect(() => {
    if (tauri) void invoke("voice_mic_monitor", { on: true, mic: params }).catch(() => {});
  }, [tauri, params]);

  useEffect(() => {
    if (!tauri) return;
    const offMeter = listen<MicMeter>("voice:mic-meter", (e) => setMeter(e.payload));
    const offTest = listen<{ phase: MicTestPhase; progress: number }>("voice:mic-test", (e) => {
      if (e.payload.phase === "done" || e.payload.phase === "error") return;
      setPhase(e.payload.phase);
      setProgress(e.payload.progress);
    });
    return () => {
      void offMeter.then((f) => f());
      void offTest.then((f) => f());
    };
  }, [tauri]);

  useDemoMeter(!tauri, setMeter);

  const level = useMotionValue(0);
  const floor = useMotionValue(0);
  const levelSpring = useSpring(level, { stiffness: 420, damping: 34, mass: 0.6 });
  const shownLevel = reduced ? level : levelSpring;
  const floorLeft = useTransform(floor, (f) => `${f * 100}%`);
  useEffect(() => {
    level.set(meterFraction(meter?.db ?? -90));
    floor.set(meterFraction(meter?.floorDb ?? -90));
  }, [meter, level, floor]);

  const running = phase === "silence" || phase === "speech" || phase === "analyzing";

  const runTest = async () => {
    setResult(null);
    setError(null);
    setPhase("silence");
    setProgress(0);
    if (!tauri) {
      // Browser preview: walk the phases and show a sample result.
      for (const [p, ms] of [["silence", 2000], ["speech", 4500], ["analyzing", 700]] as const) {
        setPhase(p);
        const start = performance.now();
        while (performance.now() - start < ms) {
          setProgress((performance.now() - start) / ms);
          await new Promise((r) => window.setTimeout(r, 80));
        }
      }
      setResult(DEMO_RESULT);
      setPhase("done");
      return;
    }
    try {
      const r = await invoke<MicTestResult>("voice_mic_test", { mic: params });
      setResult(r);
      setPhase("done");
    } catch (e) {
      setError(errorMessage(e));
      setPhase("error");
    }
  };

  const total = testProgress(phase, progress);

  return (
    <Group title="Микрофон">
      <Row
        label="Чувствительность микрофона"
        hint="«Авто» подстраивает усиление под ваш голос, пока вы говорите, и запоминает его для каждого микрофона."
      >
        <Segmented
          label="Чувствительность микрофона"
          value={value.micGainMode}
          options={[
            { id: "auto", label: "Авто" },
            { id: "manual", label: "Вручную" },
          ]}
          onChange={(v) => onChange({ micGainMode: v })}
        />
      </Row>
      <AnimatePresence initial={false}>
        {value.micGainMode === "manual" && (
          <motion.div
            key="gain"
            initial={reduced ? false : { opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={reduced ? { opacity: 0 } : { opacity: 0, height: 0 }}
            transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            style={{ overflow: "hidden" }}
          >
            <Row label="Усиление" hint="От −6 до +30 дБ. Тихому микрофону нужно больше, громкому — меньше.">
              <div className="inline">
                <input
                  className="slider"
                  type="range"
                  min={-6}
                  max={30}
                  step={1}
                  value={clampGain(value.micGainDb)}
                  aria-label="Усиление микрофона"
                  onChange={(e) => onChange({ micGainDb: Number(e.target.value) })}
                />
                <span className="slider-readout mono">{formatGain(value.micGainDb)}</span>
              </div>
            </Row>
          </motion.div>
        )}
      </AnimatePresence>
      <Row
        label="Шумоподавление"
        hint="Обычное убирает гул и шипение, но оставляет распознаванию немного живого звука. Сильное — для шумной комнаты."
      >
        <Segmented
          label="Шумоподавление"
          value={value.noiseSuppression}
          options={[
            { id: "off", label: "Выкл" },
            { id: "auto", label: "Обычное" },
            { id: "strong", label: "Сильное" },
          ]}
          onChange={(v) => onChange({ noiseSuppression: v })}
        />
      </Row>

      <div className="mic-check">
        <div className="mic-check-head">
          <span className="mic-check-title">
            Что слышит Джарвис
            {meter?.device && !meter.error && <span className="mic-check-device">{meter.device}</span>}
          </span>
          <span className="mic-check-read mono">
            {meter?.error ? (
              <span className="mic-check-err">{meter.error}</span>
            ) : (
              <>
                <span>
                  уровень <b>{formatDb(meter?.db ?? -90)}</b>
                </span>
                <span>
                  шум <b>{formatDb(meter?.floorDb ?? -90)}</b>
                </span>
                <span>
                  усиление <b>{formatGain(meter?.gainDb ?? 0)}</b>
                </span>
              </>
            )}
          </span>
        </div>
        <div className={`mic-meter${meter?.speech ? " is-speech" : ""}`} role="meter" aria-label="Уровень микрофона после обработки" aria-valuemin={-75} aria-valuemax={-5} aria-valuenow={Math.round(meter?.db ?? -75)}>
          <span className="mic-meter-target" style={{ left: `${meterFraction(-20) * 100}%` }} title="Где должна быть речь" />
          <motion.span className="mic-meter-fill" style={{ scaleX: shownLevel }} />
          <motion.span className="mic-meter-floor" style={{ left: floorLeft }} title="Шум комнаты">
            <i>шум</i>
          </motion.span>
        </div>

        <div className="mic-test">
          <button className="btn btn-primary" disabled={running} onClick={() => void runTest()}>
            {running ? "Слушаю…" : result || error ? "Проверить ещё раз" : "Проверить микрофон"}
          </button>
          <AnimatePresence mode="wait" initial={false}>
            {running && (
              <motion.div
                key={phase}
                className="mic-test-prompt"
                initial={reduced ? false : { opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.18 }}
              >
                <span className={`mic-test-step${phase === "speech" ? " is-say" : ""}`}>{phasePrompt(phase)}</span>
                <span className="mic-test-bar">
                  <span style={{ transform: `scaleX(${total})` }} />
                </span>
              </motion.div>
            )}
            {!running && error && (
              <motion.div key="err" className="mic-test-prompt mic-check-err" initial={reduced ? false : { opacity: 0 }} animate={{ opacity: 1 }}>
                {error}
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        <AnimatePresence initial={false}>
          {result && !running && (
            <motion.div
              key="result"
              className={`mic-result is-${result.severity}`}
              initial={reduced ? false : { opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
            >
              <div className="mic-result-advice">{result.advice}</div>
              <div className="mic-result-grid">
                <div>
                  <span>Шум комнаты</span>
                  <b className="mono">{formatDb(result.noiseFloorDb)}</b>
                </div>
                <div>
                  <span>Речь</span>
                  <b className="mono">{formatDb(result.speechLevelDb)}</b>
                </div>
                <div>
                  <span>Сигнал/шум</span>
                  <b className="mono">
                    {Math.round(result.snrDb)} дБ <em>{snrWord(result.snrDb)}</em>
                  </b>
                </div>
                <div>
                  <span>Усиление</span>
                  <b className="mono">{formatGain(result.appliedGainDb)}</b>
                </div>
              </div>
              <div className="mic-result-heard">
                <span>Распознано</span>
                {result.sttAvailable ? (
                  <q>{result.transcript || "ничего не разобрал"}</q>
                ) : (
                  <em>модель распознавания не скачана</em>
                )}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </Group>
  );
}
