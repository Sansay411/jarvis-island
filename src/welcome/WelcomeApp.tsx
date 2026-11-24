// First-run «Знакомство» flow: a calm, 6-step introduction so JARVIS knows
// who it is talking to, what matters to them, their daily rhythm, and their
// microphone from the very first minute. Every step is skippable.

import { AnimatePresence, motion, useMotionValue, useReducedMotion, useSpring, type MotionValue } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { hasTauri } from "../lib/workspace-ipc";
import { Eyes, type EyeMood } from "../island/Eyes";
import type { Emote } from "../store/emote";
import { TitleBar } from "../settings/TitleBar";
import type { Settings } from "../settings/SettingsApp";
import { Icon } from "../ui/Icon";
import {
  GOAL_OPTIONS,
  OCCUPATION_OPTIONS,
  STYLE_OPTIONS,
  answersFromSettings,
  applyOnboardingToSettings,
  buildMemoryFacts,
  mapGoalsToMentorTracks,
  type GoalId,
  type OnboardingAnswers,
  type OnboardingStep,
} from "./onboarding";
import "./welcome.css";

interface AudioDevices {
  inputs: string[];
  outputs: string[];
  defaultInput: string | null;
  defaultOutput: string | null;
}

interface SpeechModel {
  id: string;
  kind: "stt" | "vad" | "tts";
  installed: boolean;
}

interface SettingsView {
  settings: Settings;
}

const FALLBACK_SETTINGS: Settings = {
  provider: { preset: "jarvis-cloud", baseUrl: "", model: "deepseek-flash", visionModel: "", temperature: 0.3 },
  voice: {
    enabled: true,
    engine: "local",
    sttModel: "gigaam-v3",
    ttsVoice: "edge:ru-RU-DmitryNeural",
    ttsRate: 1.1,
    ttsPitch: 0,
    autoSpeakReplies: false,
    wakeWord: false,
    realtimeUrl: "",
    model: "",
    voice: "",
    reuseLlmKey: true,
    inputDevice: "",
    outputDevice: "",
    duckMedia: true,
    cloudStt: true,
  },
  persona: { userName: "", address: "ty", style: "witty" },
  shortcuts: { workspace: "Ctrl+Alt+J", voice: "Ctrl+Alt+K", clipboard: "Ctrl+Alt+V" },
  privacy: { storeHistory: true, allowScreen: true, allowClipboardRead: false },
  clipboard: { enabled: true, maxItems: 100, keepDays: 7, skipSecrets: true, ignoredApps: [] },
  agent: { maxSteps: 24, confirmModerate: true, fullControl: false, projectRoots: [], learnSkills: true },
  launchAtStartup: false,
  reduceMotion: false,
  updates: { autoCheck: true },
  planner: { workStart: "09:00", workEnd: "18:00", dayStartHour: 8, googleClientId: "", googleCalendarId: "primary", islandRemind: 10 },
  mentor: {
    guard: true,
    shortMinutes: 10,
    youtubeMinutes: 30,
    earn: true,
    strictFocus: true,
    speak: true,
    tracks: ["code", "english", "mind", "body", "projects"],
    learningWords: [],
    nightFrom: "",
    nightTo: "",
    ifThen: "",
  },
  telegram: { enabled: true, chatId: 0, botUsername: "", morning: "08:30", evening: "21:30", remindBefore: 10, agent: true },
  onboarding: { done: false, version: 0, dismissedCount: 0 },
};

const FALLBACK_DEVICES: AudioDevices = {
  inputs: ["Микрофон (USB Audio)", "Гарнитура (Bluetooth Hands-Free)"],
  outputs: ["Динамики (Realtek Audio)"],
  defaultInput: "Микрофон (USB Audio)",
  defaultOutput: "Динамики (Realtek Audio)",
};

const TRACK_LABELS: Record<string, string> = {
  code: "код",
  english: "английский",
  mind: "логика",
  body: "спорт",
  projects: "проекты",
};

const TOTAL_STEPS = 6; // steps 0..5, then step 6 = «Готово»

function Keycaps({ value }: { value: string }) {
  const keys = value
    .split("+")
    .map((k) => k.trim())
    .filter(Boolean);
  if (keys.length === 0) return null;
  return (
    <span className="w-keycaps" aria-hidden="true">
      {keys.map((k, i) => (
        <kbd key={`${k}-${i}`}>{k}</kbd>
      ))}
    </span>
  );
}

/** 16-bar live microphone level visualiser driven without React re-renders. */
function MicMeter({ level, active }: { level: MotionValue<number>; active: boolean }) {
  const barsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const update = (v: number) => {
      const container = barsRef.current;
      if (!container) return;
      const clamped = Math.max(0, Math.min(1, v));
      const children = container.children;
      const n = children.length;
      for (let i = 0; i < n; i++) {
        const el = children[i] as HTMLElement;
        const threshold = (i + 0.6) / n;
        const lit = active && clamped >= threshold * 0.85;
        el.dataset.lit = lit ? "1" : "0";
        const weight = 0.35 + 0.65 * Math.sin(((i + 0.5) / n) * Math.PI);
        const h = active ? Math.max(4, Math.round(4 + clamped * 20 * weight)) : 4;
        el.style.height = `${h}px`;
      }
    };
    update(level.get());
    return level.on("change", update);
  }, [level, active]);

  return (
    <div className={`w-meter${active ? " is-active" : ""}`} ref={barsRef} aria-hidden="true">
      {Array.from({ length: 16 }, (_, i) => (
        <span key={i} className="w-meter-bar" data-lit="0" />
      ))}
    </div>
  );
}

export interface WelcomeAppProps {
  initialStep?: OnboardingStep;
  qaMode?: boolean;
  onQaSaved?: (payload: { settings: Settings; facts: string[] }) => void;
}

export default function WelcomeApp({ initialStep = 0, qaMode = false, onQaSaved }: WelcomeAppProps = {}) {
  const [step, setStep] = useState<OnboardingStep>(initialStep);
  const [dir, setDir] = useState<1 | -1>(1);
  const [baseSettings, setBaseSettings] = useState<Settings>(FALLBACK_SETTINGS);
  const [answers, setAnswers] = useState<OnboardingAnswers>(() => answersFromSettings(FALLBACK_SETTINGS));
  const [skipped, setSkipped] = useState<Set<number>>(() => new Set());
  const [devices, setDevices] = useState<AudioDevices>(FALLBACK_DEVICES);
  const [modelsReady, setModelsReady] = useState<boolean>(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Voice test state on Step 5.
  const [micTesting, setMicTesting] = useState(false);
  const [micStatus, setMicStatus] = useState<string | null>(null);
  const [micHint, setMicHint] = useState<string | null>(null);
  const [heardText, setHeardText] = useState<string>("");
  const micLevel = useMotionValue(0);
  const browserMicCleanup = useRef<(() => void) | null>(null);
  const qaSimTimers = useRef<number[]>([]);

  // Eye presence: gaze + reactive mood & one-shot emotes.
  const notchRef = useRef<HTMLDivElement>(null);
  const lookX = useSpring(0, { stiffness: 160, damping: 22 });
  const [lookY, setLookY] = useState(0);
  const [eyeMood, setEyeMood] = useState<EyeMood>("attentive");
  const [eyeEmote, setEyeEmote] = useState<Emote | null>("wake");
  const [eyeEmoteSeq, setEyeEmoteSeq] = useState(1);
  const reactTimer = useRef<number | undefined>(undefined);
  const reduced = Boolean(useReducedMotion() || baseSettings.reduceMotion);

  const pulseHappy = useCallback((kind?: Emote) => {
    window.clearTimeout(reactTimer.current);
    setEyeMood("happy");
    if (kind) {
      setEyeEmote(kind);
      setEyeEmoteSeq((n) => n + 1);
    }
    reactTimer.current = window.setTimeout(() => {
      setEyeMood("attentive");
      setEyeEmote(null);
    }, kind ? 1200 : 950);
  }, []);

  // Sync external initialStep changes in QA harness.
  useEffect(() => {
    setStep(initialStep);
    if (initialStep === 6) {
      setEyeMood("happy");
      setEyeEmote("star");
      setEyeEmoteSeq((n) => n + 1);
    } else if (initialStep === 0) {
      setEyeMood("attentive");
      setEyeEmote("wake");
      setEyeEmoteSeq((n) => n + 1);
    } else {
      setEyeMood("attentive");
      setEyeEmote(null);
    }
  }, [initialStep]);

  // Load real settings, audio devices, and STT model readiness in Tauri.
  useEffect(() => {
    if (!hasTauri() || qaMode) return;
    let active = true;
    void invoke<SettingsView>("settings_get")
      .then((view) => {
        if (!active || !view?.settings) return;
        const merged: Settings = {
          ...FALLBACK_SETTINGS,
          ...view.settings,
          onboarding: view.settings.onboarding ?? FALLBACK_SETTINGS.onboarding,
        };
        setBaseSettings(merged);
        setAnswers(answersFromSettings(merged));
      })
      .catch(() => {});
    void invoke<AudioDevices>("audio_devices")
      .then((d) => active && d && setDevices(d))
      .catch(() => {});
    void invoke<SpeechModel[]>("stt_models")
      .then((list) => {
        if (!active || !Array.isArray(list)) return;
        const stt = list.filter((m) => m.kind === "stt" || m.kind === "vad");
        if (stt.length > 0) setModelsReady(stt.every((m) => m.installed));
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [qaMode]);

  // Gaze tracks the pointer across the welcome window.
  useEffect(() => {
    if (reduced) {
      lookX.set(0);
      setLookY(0);
      return;
    }
    let raf = 0;
    const onMove = (e: PointerEvent) => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const r = notchRef.current?.getBoundingClientRect();
        if (!r) return;
        const cx = r.left + r.width / 2;
        const cy = r.top + r.height / 2;
        lookX.set(Math.max(-1, Math.min(1, (e.clientX - cx) / 360)));
        setLookY(Math.round(Math.max(-2, Math.min(3, (e.clientY - cy) / 140))));
      });
    };
    window.addEventListener("pointermove", onMove);
    return () => {
      window.removeEventListener("pointermove", onMove);
      cancelAnimationFrame(raf);
    };
  }, [lookX, reduced]);

  const stopMicTest = useCallback(() => {
    for (const t of qaSimTimers.current) window.clearTimeout(t);
    qaSimTimers.current = [];
    browserMicCleanup.current?.();
    browserMicCleanup.current = null;
    setMicTesting(false);
    setMicStatus(null);
    micLevel.set(0);
    if (hasTauri() && !qaMode) {
      void invoke("voice_stop").catch(() => {});
    }
  }, [micLevel, qaMode]);

  // Clean up mic test when leaving step 5 or unmounting.
  useEffect(() => {
    if (step !== 5 && micTesting) {
      stopMicTest();
    }
  }, [step, micTesting, stopMicTest]);

  useEffect(() => () => stopMicTest(), [stopMicTest]);

  // Listen to native voice events while testing microphone on Step 5.
  useEffect(() => {
    if (!micTesting || !hasTauri() || qaMode) return;
    let alive = true;
    const unsubs = [
      listen<{ input: number; output: number }>("voice:level", (e) => {
        if (alive) micLevel.set(e.payload.input);
      }),
      listen<{ role: "user" | "assistant"; text: string; final: boolean }>("voice:transcript", (e) => {
        if (!alive || e.payload.role !== "user") return;
        const text = e.payload.text.trim();
        if (!text) return;
        setHeardText((prev) => (e.payload.final ? text : prev ? `${prev} ${text}` : text));
        if (e.payload.final) {
          pulseHappy("heart");
        }
      }),
      listen<{ state: string; error: string | null }>("voice:state", (e) => {
        if (!alive) return;
        const { state, error: err } = e.payload;
        if (state === "connecting") setMicStatus("Подключаю микрофон…");
        else if (state === "listening") setMicStatus("Слушаю — скажите фразу");
        else if (state === "hearing") setMicStatus("Слышу вас…");
        else if (state === "thinking" || state === "speaking") setMicStatus("Распознано");
        else if (state === "error") {
          setMicHint(err || "Не удалось открыть микрофон");
          setMicTesting(false);
          micLevel.set(0);
        }
      }),
    ];
    return () => {
      alive = false;
      unsubs.forEach((p) => void p.then((off) => off()));
    };
  }, [micTesting, micLevel, pulseHappy, qaMode]);

  // Browser Web Audio fallback for live level meter when native STT isn't ready.
  const startBrowserMeter = useCallback(async (): Promise<boolean> => {
    if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) return false;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const ctx = new AudioContext();
      const src = ctx.createMediaStreamSource(stream);
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 512;
      src.connect(analyser);
      const buf = new Float32Array(analyser.fftSize);
      let raf = 0;
      const tick = () => {
        analyser.getFloatTimeDomainData(buf);
        let sum = 0;
        for (const v of buf) sum += v * v;
        const rms = Math.sqrt(sum / buf.length);
        const db = 20 * Math.log10(Math.max(rms, 1e-5));
        micLevel.set(Math.max(0, Math.min(1, (db + 52) / 42)));
        raf = requestAnimationFrame(tick);
      };
      raf = requestAnimationFrame(tick);
      browserMicCleanup.current = () => {
        cancelAnimationFrame(raf);
        stream.getTracks().forEach((t) => t.stop());
        void ctx.close().catch(() => {});
      };
      return true;
    } catch {
      return false;
    }
  }, [micLevel]);

  const startMicTest = useCallback(async () => {
    setMicHint(null);
    setHeardText("");
    setMicTesting(true);

    if (!hasTauri() || qaMode) {
      setMicStatus("Слушаю — скажите фразу");
      let t = 0;
      const iv = window.setInterval(() => {
        t += 1;
        const wave = 0.22 + 0.58 * Math.abs(Math.sin(t * 0.42)) * Math.abs(Math.cos(t * 0.19));
        micLevel.set(wave);
      }, 65);
      qaSimTimers.current.push(iv);
      const done = window.setTimeout(() => {
        setHeardText("Джарвис, привет");
        setMicStatus("Отлично слышно!");
        pulseHappy("heart");
      }, 1400);
      qaSimTimers.current.push(done);
      return;
    }

    if (!baseSettings.voice.enabled) {
      const ok = await startBrowserMeter();
      setMicStatus(ok ? "Микрофон работает" : null);
      setMicHint("Голосовой режим сейчас выключен — включить его можно в Настройках → Голос.");
      if (!ok) setMicTesting(false);
      return;
    }

    if (!modelsReady) {
      const ok = await startBrowserMeter();
      setMicStatus(ok ? "Микрофон работает" : null);
      setMicHint("Речевая модель ещё не скачана: уровень виден уже сейчас, а распознавание заработает после загрузки в Настройках → Голос.");
      if (!ok) setMicTesting(false);
      return;
    }

    try {
      // Save chosen input device first so VoiceManager opens the selected mic.
      if (answers.inputDevice !== baseSettings.voice.inputDevice) {
        const next = structuredClone(baseSettings);
        next.voice.inputDevice = answers.inputDevice;
        await invoke("settings_save", { settings: next });
        setBaseSettings(next);
      }
      setMicStatus("Подключаю микрофон…");
      await invoke("voice_toggle");
    } catch (e) {
      const ok = await startBrowserMeter();
      setMicHint(errorMessage(e));
      if (!ok) {
        setMicTesting(false);
        setMicStatus(null);
      } else {
        setMicStatus("Микрофон работает (проверка уровня)");
      }
    }
  }, [answers.inputDevice, baseSettings, micLevel, modelsReady, pulseHappy, qaMode, startBrowserMeter]);

  const goTo = useCallback(
    (next: OnboardingStep, direction?: 1 | -1) => {
      setError(null);
      setDir(direction ?? (next >= step ? 1 : -1));
      setStep(next);
      if (next === 6) {
        setEyeMood("happy");
        setEyeEmote("star");
        setEyeEmoteSeq((n) => n + 1);
      }
    },
    [step],
  );

  const commitAll = useCallback(
    async (skippedFinal: Set<number>) => {
      stopMicTest();
      setSaving(true);
      setError(null);
      try {
        const updatedSettings = applyOnboardingToSettings(baseSettings, answers, skippedFinal);
        const facts = buildMemoryFacts(answers, skippedFinal);
        if (hasTauri() && !qaMode) {
          await invoke("settings_save", { settings: updatedSettings });
          for (const text of facts) {
            await invoke("memory_add", { text });
          }
        }
        setBaseSettings(updatedSettings);
        onQaSaved?.({ settings: updatedSettings, facts });
        goTo(6, 1);
      } catch (e) {
        setError(`Не удалось сохранить настройки: ${errorMessage(e)}`);
      } finally {
        setSaving(false);
      }
    },
    [answers, baseSettings, goTo, onQaSaved, qaMode, stopMicTest],
  );

  const handleNext = useCallback(() => {
    if (saving) return;
    const nextSkipped = new Set(skipped);
    nextSkipped.delete(step);
    setSkipped(nextSkipped);

    if (step < 5) {
      goTo((step + 1) as OnboardingStep, 1);
    } else if (step === 5) {
      void commitAll(nextSkipped);
    }
  }, [commitAll, goTo, saving, skipped, step]);

  const handleSkip = useCallback(() => {
    if (saving) return;
    const nextSkipped = new Set(skipped);
    nextSkipped.add(step);
    setSkipped(nextSkipped);

    if (step < 5) {
      goTo((step + 1) as OnboardingStep, 1);
    } else if (step === 5) {
      void commitAll(nextSkipped);
    }
  }, [commitAll, goTo, saving, skipped, step]);

  const handleBack = useCallback(() => {
    if (saving || step <= 0 || step >= 6) return;
    goTo((step - 1) as OnboardingStep, -1);
  }, [goTo, saving, step]);

  const handleLater = useCallback(() => {
    stopMicTest();
    if (hasTauri() && !qaMode) {
      void invoke("welcome_dismiss").catch(() => {});
    }
  }, [qaMode, stopMicTest]);

  const handleFinish = useCallback(() => {
    stopMicTest();
    if (hasTauri() && !qaMode) {
      void invoke("welcome_complete").catch(() => {});
    }
  }, [qaMode, stopMicTest]);

  // Keyboard navigation: Enter advances, Alt+ArrowLeft steps back.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing || saving) return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (e.key === "Enter" && tag !== "BUTTON" && tag !== "SELECT" && tag !== "TEXTAREA") {
        e.preventDefault();
        if (step === 6) handleFinish();
        else handleNext();
      } else if (e.altKey && e.key === "ArrowLeft" && step > 0 && step < 6) {
        e.preventDefault();
        handleBack();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [handleBack, handleFinish, handleNext, saving, step]);

  const vy = answers.address === "vy";
  const trimmedName = answers.userName.trim();
  const mappedTracks = useMemo(() => mapGoalsToMentorTracks(answers.goals), [answers.goals]);
  const activeOccupation = OCCUPATION_OPTIONS.find((o) => o.id === answers.occupation);

  const toggleGoal = (id: GoalId) => {
    setAnswers((a) => {
      const has = a.goals.includes(id);
      const goals = has ? a.goals.filter((g) => g !== id) : [...a.goals, id];
      if (!has) pulseHappy();
      return { ...a, goals };
    });
  };

  const effectiveMood: EyeMood = step === 5 && micTesting ? "listening" : eyeMood;
  const stillEyes = effectiveMood === "listening" || effectiveMood === "speaking" || reduced;

  const slideVariants = {
    enter: (d: number) => (reduced ? { opacity: 0 } : { opacity: 0, x: d * 24 }),
    center: { opacity: 1, x: 0 },
    exit: (d: number) => (reduced ? { opacity: 0 } : { opacity: 0, x: -d * 24 }),
  };

  return (
    <div className="welcome">
      <TitleBar allowMaximize={false} onClose={step === 6 ? handleFinish : handleLater} />

      <header className="w-head" data-tauri-drag-region>
        <div className="w-notch" ref={notchRef}>
          <Eyes
            mood={effectiveMood}
            level={step === 5 && micTesting ? micLevel : undefined}
            lookX={stillEyes ? undefined : lookX}
            lookY={stillEyes ? 0 : lookY}
            scale={1.72}
            emote={eyeEmote}
            emoteSeq={eyeEmoteSeq}
            onBoop={() => pulseHappy("boop")}
          />
        </div>

        {step < TOTAL_STEPS && (
          <nav className="w-dots" aria-label="Шаги знакомства">
            {Array.from({ length: TOTAL_STEPS }, (_, idx) => {
              const active = step === idx;
              const passed = idx < step;
              return (
                <button
                  key={idx}
                  type="button"
                  className={`w-dot${active ? " is-active" : passed ? " is-done" : ""}`}
                  aria-label={`Шаг ${idx + 1} из ${TOTAL_STEPS}`}
                  aria-current={active ? "step" : undefined}
                  onClick={() => goTo(idx as OnboardingStep)}
                />
              );
            })}
          </nav>
        )}
      </header>

      <main className="w-stage">
        <AnimatePresence mode="wait" custom={dir} initial={false}>
          <motion.section
            key={step}
            custom={dir}
            variants={slideVariants}
            initial="enter"
            animate="center"
            exit="exit"
            transition={{ duration: reduced ? 0.01 : 0.22, ease: [0.22, 1, 0.36, 1] }}
            className={`w-step w-step-${step}`}
          >
            {step === 0 && (
              <div className="w-hero">
                <h1>Привет, я Джарвис</h1>
                <p className="w-lead">
                  Живу в острове наверху экрана и помогаю с делами, планами и голосом. Давай познакомимся за минуту, чтобы я сразу говорил как надо.
                </p>
                <div className="w-pillars" aria-hidden="true">
                  <div className="w-pillar">
                    <Icon name="chat" size={16} />
                    <span>Имя и тон</span>
                  </div>
                  <div className="w-pillar">
                    <Icon name="target" size={16} />
                    <span>Цели и ритм</span>
                  </div>
                  <div className="w-pillar">
                    <Icon name="mic" size={16} />
                    <span>Голос</span>
                  </div>
                </div>
              </div>
            )}

            {step === 1 && (
              <div className="w-form">
                <div className="w-title-block">
                  <h1>{vy ? "Как к вам обращаться?" : "Как к тебе обращаться?"}</h1>
                  <p className="w-sub">Буду иногда называть по имени — без навязчивости.</p>
                </div>

                <div className="w-field-row">
                  <label className="w-label" htmlFor="w-name">
                    Имя
                  </label>
                  <input
                    id="w-name"
                    className="w-input"
                    value={answers.userName}
                    placeholder="Например, Алексей"
                    autoFocus={!qaMode}
                    spellCheck={false}
                    onChange={(e) => setAnswers((a) => ({ ...a, userName: e.target.value }))}
                    onBlur={() => {
                      if (answers.userName.trim()) pulseHappy("wink");
                    }}
                  />
                </div>

                <div className="w-field-row">
                  <span className="w-label">Обращение</span>
                  <div className="w-seg" role="radiogroup" aria-label="Обращение">
                    {(
                      [
                        { id: "ty", label: "На «ты»" },
                        { id: "vy", label: "На «вы»" },
                      ] as const
                    ).map((opt) => (
                      <button
                        key={opt.id}
                        type="button"
                        role="radio"
                        aria-checked={answers.address === opt.id}
                        className={`w-seg-btn${answers.address === opt.id ? " is-on" : ""}`}
                        onClick={() => {
                          setAnswers((a) => ({ ...a, address: opt.id }));
                          pulseHappy();
                        }}
                      >
                        {opt.label}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="w-field-col">
                  <span className="w-label">Манера общения</span>
                  <div className="w-style-grid" role="radiogroup" aria-label="Манера общения">
                    {STYLE_OPTIONS.map((opt) => {
                      const on = answers.style === opt.id;
                      return (
                        <button
                          key={opt.id}
                          type="button"
                          role="radio"
                          aria-checked={on}
                          className={`w-style-card${on ? " is-on" : ""}`}
                          onClick={() => {
                            setAnswers((a) => ({ ...a, style: opt.id }));
                            pulseHappy(opt.id === "witty" ? "wink" : undefined);
                          }}
                        >
                          <span className="w-style-title">{opt.label}</span>
                          <span className="w-style-hint">{opt.hint}</span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              </div>
            )}

            {step === 2 && (
              <div className="w-form">
                <div className="w-title-block">
                  <h1>{vy ? "Чем вы занимаетесь?" : "Чем ты занимаешься?"}</h1>
                  <p className="w-sub">Запомню в разделе «Память», чтобы лучше понимать контекст задач.</p>
                </div>

                <div className="w-chips" role="radiogroup" aria-label="Занятие">
                  {OCCUPATION_OPTIONS.map((opt) => {
                    const on = answers.occupation === opt.id;
                    return (
                      <button
                        key={opt.id}
                        type="button"
                        role="radio"
                        aria-checked={on}
                        className={`w-chip${on ? " is-on" : ""}`}
                        onClick={() => {
                          setAnswers((a) => ({ ...a, occupation: a.occupation === opt.id ? null : opt.id }));
                          if (!on) pulseHappy("determined");
                        }}
                      >
                        {opt.label}
                      </button>
                    );
                  })}
                </div>

                <div className="w-field-col">
                  <label className="w-label" htmlFor="w-occupation-detail">
                    Коротко о главном <span className="w-opt">(необязательно)</span>
                  </label>
                  <input
                    id="w-occupation-detail"
                    className="w-input"
                    value={answers.occupationDetail}
                    placeholder={activeOccupation?.placeholder ?? "Например: разработка на Rust, дизайн, учёба…"}
                    onChange={(e) => setAnswers((a) => ({ ...a, occupationDetail: e.target.value }))}
                  />
                </div>
              </div>
            )}

            {step === 3 && (
              <div className="w-form">
                <div className="w-title-block">
                  <h1>Что сейчас важнее всего?</h1>
                  <p className="w-sub">Можно выбрать несколько — подстрою советы и треки наставника.</p>
                </div>

                <div className="w-chips w-chips-multi" role="group" aria-label="Цели и интересы">
                  {GOAL_OPTIONS.map((g) => {
                    const on = answers.goals.includes(g.id);
                    return (
                      <button
                        key={g.id}
                        type="button"
                        aria-pressed={on}
                        className={`w-chip${on ? " is-on" : ""}`}
                        onClick={() => toggleGoal(g.id)}
                      >
                        {on && <Icon name="check" size={13} className="w-chip-check" />}
                        <span>{g.label}</span>
                      </button>
                    );
                  })}
                </div>

                {mappedTracks && (
                  <p className="w-meta">
                    Треки наставника: {mappedTracks.map((t) => TRACK_LABELS[t] ?? t).join(", ")}.
                  </p>
                )}
              </div>
            )}

            {step === 4 && (
              <div className="w-form">
                <div className="w-title-block">
                  <h1>{vy ? "Ваш ритм дня" : "Твой ритм дня"}</h1>
                  <p className="w-sub">Поможет планировать глубокую работу и не отвлекать не вовремя.</p>
                </div>

                <div className="w-rhythm-card">
                  <div className="w-rhythm-row">
                    <div>
                      <div className="w-rhythm-title">Рабочие или учебные часы</div>
                      <div className="w-rhythm-hint">Сохранится в настройках планировщика</div>
                    </div>
                    <div className="w-time-pair">
                      <input
                        type="time"
                        className="w-time"
                        aria-label="Начало рабочего дня"
                        value={answers.workStart}
                        onChange={(e) => {
                          setAnswers((a) => ({ ...a, workStart: e.target.value }));
                          pulseHappy();
                        }}
                      />
                      <span className="w-dash">—</span>
                      <input
                        type="time"
                        className="w-time"
                        aria-label="Конец рабочего дня"
                        value={answers.workEnd}
                        onChange={(e) => {
                          setAnswers((a) => ({ ...a, workEnd: e.target.value }));
                          pulseHappy();
                        }}
                      />
                    </div>
                  </div>

                  <div className="w-rhythm-row">
                    <div>
                      <div className="w-rhythm-title">Обычный подъём и сон</div>
                      <div className="w-rhythm-hint">Запомню в «Памяти» для режима дня</div>
                    </div>
                    <div className="w-time-pair">
                      <input
                        type="time"
                        className="w-time"
                        aria-label="Время подъёма"
                        value={answers.wakeTime}
                        onChange={(e) => setAnswers((a) => ({ ...a, wakeTime: e.target.value }))}
                      />
                      <span className="w-dash">—</span>
                      <input
                        type="time"
                        className="w-time"
                        aria-label="Время сна"
                        value={answers.sleepTime}
                        onChange={(e) => setAnswers((a) => ({ ...a, sleepTime: e.target.value }))}
                      />
                    </div>
                  </div>
                </div>
              </div>
            )}

            {step === 5 && (
              <div className="w-form">
                <div className="w-title-block">
                  <h1>Проверим голос</h1>
                  <p className="w-sub">Можно выбрать микрофон и сразу убедиться, что Джарвис хорошо слышит.</p>
                </div>

                <div className="w-field-row">
                  <label className="w-label" htmlFor="w-mic">
                    Микрофон
                  </label>
                  <select
                    id="w-mic"
                    className="w-select"
                    value={answers.inputDevice}
                    onChange={(e) => setAnswers((a) => ({ ...a, inputDevice: e.target.value }))}
                  >
                    <option value="">По умолчанию{devices.defaultInput ? ` (${devices.defaultInput})` : ""}</option>
                    {devices.inputs.map((d) => (
                      <option key={d} value={d}>
                        {d}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="w-voice-box">
                  <div className="w-voice-top">
                    <div>
                      <div className="w-voice-prompt">Скажите: «Джарвис, привет»</div>
                      <div className="w-voice-state">
                        {micStatus ?? (micTesting ? "Слушаю…" : "Нажмите кнопку, чтобы проверить звук")}
                      </div>
                    </div>
                    <button
                      type="button"
                      className={`w-btn ${micTesting ? "w-btn-quiet" : "w-btn-primary"}`}
                      onClick={() => (micTesting ? stopMicTest() : void startMicTest())}
                    >
                      <Icon name="mic" size={14} />
                      <span>{micTesting ? "Остановить" : "Проверить"}</span>
                    </button>
                  </div>

                  <MicMeter level={micLevel} active={micTesting} />

                  {heardText && (
                    <div className="w-heard" role="status">
                      <Icon name="check" size={13} />
                      <span>Услышал: «{heardText}»</span>
                    </div>
                  )}
                  {micHint && <p className="w-voice-hint">{micHint}</p>}
                </div>

                <div className="w-switch-row">
                  <div>
                    <div className="w-rhythm-title">Откликаться на «Джарвис, …»</div>
                    <div className="w-rhythm-hint">Держать микрофон готовым к обращению по имени</div>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={answers.wakeWord}
                    aria-label="Откликаться на имя"
                    className={`w-switch${answers.wakeWord ? " is-on" : ""}`}
                    onClick={() => {
                      setAnswers((a) => ({ ...a, wakeWord: !a.wakeWord }));
                      pulseHappy();
                    }}
                  >
                    <span />
                  </button>
                </div>
              </div>
            )}

            {step === 6 && (
              <div className="w-done">
                <div className="w-title-block is-center">
                  <h1>{trimmedName ? `Готово, ${trimmedName}!` : "Всё готово!"}</h1>
                  <p className="w-sub">
                    {vy ? "Я запомнил ваши предпочтения. Три короткие подсказки на старт:" : "Я всё запомнил. Три короткие подсказки на старт:"}
                  </p>
                </div>

                <ul className="w-tips">
                  <li>
                    <span className="w-tip-icon">
                      <Icon name="chat" size={16} />
                    </span>
                    <div className="w-tip-body">
                      <strong>Быстрый чат</strong>
                      <span>Открыть или свернуть окно разговора с любого экрана</span>
                    </div>
                    <Keycaps value={baseSettings.shortcuts.workspace} />
                  </li>
                  <li>
                    <span className="w-tip-icon">
                      <Icon name="mic" size={16} />
                    </span>
                    <div className="w-tip-body">
                      <strong>Голосом: «Джарвис, …»</strong>
                      <span>{answers.wakeWord ? "Зовите по имени или включайте клавишей" : "Включайте голосовой режим клавишей или кнопкой на острове"}</span>
                    </div>
                    <Keycaps value={baseSettings.shortcuts.voice} />
                  </li>
                  <li>
                    <span className="w-tip-icon">
                      <Icon name="settings" size={16} />
                    </span>
                    <div className="w-tip-body">
                      <strong>Настройки и память</strong>
                      <span>Иконка в трее или шестерёнка на острове — там можно изменить всё</span>
                    </div>
                  </li>
                </ul>
              </div>
            )}
          </motion.section>
        </AnimatePresence>
      </main>

      {error && (
        <div className="w-error" role="alert">
          {error}
        </div>
      )}

      <footer className="w-foot">
        {step === 0 ? (
          <>
            <button type="button" className="w-btn w-btn-ghost" onClick={handleLater}>
              Позже
            </button>
            <button type="button" className="w-btn w-btn-primary" onClick={handleNext}>
              Познакомиться
            </button>
          </>
        ) : step < 6 ? (
          <>
            <button type="button" className="w-btn w-btn-ghost" disabled={saving} onClick={handleBack}>
              Назад
            </button>
            <div className="w-foot-right">
              <button type="button" className="w-btn w-btn-ghost" disabled={saving} onClick={handleSkip}>
                Пропустить
              </button>
              <button type="button" className="w-btn w-btn-primary" disabled={saving} onClick={handleNext}>
                {saving ? "Сохраняю…" : step === 5 ? "Сохранить" : "Дальше"}
              </button>
            </div>
          </>
        ) : (
          <div className="w-foot-center">
            <button type="button" className="w-btn w-btn-primary w-btn-wide" onClick={handleFinish}>
              Готово
            </button>
          </div>
        )}
      </footer>
    </div>
  );
}
