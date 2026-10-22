// Settings: JARVIS's own room. The face from the island lives in the left
// rail — its eyes follow the pointer, become real audio bars while a voice
// sample plays, and smile when changes are saved. Sections speak in
// JARVIS's voice; amber marks only JARVIS's presence (eyes, the active
// section, focus). Secrets go straight to Windows Credential Manager through
// `secret_set`; the UI only ever learns whether a key exists.

import { AnimatePresence, motion, useMotionValue, useSpring, type MotionValue } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { Eyes, type EyeMood } from "../island/Eyes";
import "./settings.css";

interface Preset {
  id: string;
  label: string;
  baseUrl: string;
  defaultModel: string;
  needsKey: boolean;
  dashscope: boolean;
}

interface Settings {
  provider: { preset: string; baseUrl: string; model: string; visionModel: string; temperature: number };
  voice: {
    enabled: boolean;
    engine: "local" | "realtime";
    sttModel: string;
    ttsVoice: string;
    ttsRate: number;
    wakeWord: boolean;
    realtimeUrl: string;
    model: string;
    voice: string;
    reuseLlmKey: boolean;
    inputDevice: string;
    outputDevice: string;
  };
  persona: { userName: string; address: "ty" | "vy"; style: "witty" | "warm" | "concise" };
  shortcuts: { workspace: string; voice: string };
  privacy: { storeHistory: boolean; allowScreen: boolean; allowClipboardRead: boolean };
  agent: { maxSteps: number; confirmModerate: boolean; projectRoots: string[] };
  launchAtStartup: boolean;
  reduceMotion: boolean;
}

interface SettingsView {
  settings: Settings;
  presets: Preset[];
  hasLlmKey: boolean;
  hasVoiceKey: boolean;
  logPath: string | null;
  dataPath: string | null;
}

interface Memory {
  id: number;
  text: string;
  createdAt: string;
}

interface AudioDevices {
  inputs: string[];
  outputs: string[];
  defaultInput: string | null;
  defaultOutput: string | null;
}

interface VoiceStatus {
  state: string;
  metrics: { lastTimeToFirstAudioMs: number | null; lastBargeInMs: number | null; connectMs: number | null; lastSttMs: number | null };
}

interface SpeechModel {
  id: string;
  kind: "stt" | "vad" | "tts";
  label: string;
  size: number;
  installed: boolean;
}

interface ModelProgress {
  id: string;
  done: number;
  total: number;
  state: "downloading" | "done" | "cancelled" | "error";
  error: string | null;
}

interface TtsVoice {
  id: string;
  name: string;
  language: string;
}

interface TaskRecord {
  id: string;
  request: string;
  status: string;
  steps: number;
  error: string | null;
  startedAt: string;
}

interface Diagnostics {
  uptimeSecs: number;
  trackerHz: number;
  workingSetMb: number;
  monitors: { name: string; primary: boolean; scale: number; bounds: { width: number; height: number } }[];
}

type Section = "persona" | "voice" | "model" | "behaviour" | "shortcuts" | "privacy" | "memory" | "diagnostics";
type Tone = "ok" | "error" | "info";

/** Section titles, and the line JARVIS says about each (ты / вы). */
const SECTIONS: { id: Section; label: string; line: [string, string] }[] = [
  { id: "persona", label: "Характер", line: ["Как мне к тебе обращаться и каким быть в разговоре.", "Как мне к вам обращаться и каким быть в разговоре."] },
  { id: "voice", label: "Голос", line: ["Так я тебя слышу и так отвечаю. Речь не покидает этот компьютер.", "Так я вас слышу и так отвечаю. Речь не покидает этот компьютер."] },
  { id: "model", label: "Модель", line: ["Здесь решается, чем я думаю. Без модели я умею только моргать.", "Здесь решается, чем я думаю. Без модели я умею только моргать."] },
  { id: "behaviour", label: "Поведение", line: ["Где я спрашиваю разрешения, а где действую сам.", "Где я спрашиваю разрешения, а где действую сам."] },
  { id: "shortcuts", label: "Сочетания клавиш", line: ["Как позвать меня с клавиатуры.", "Как позвать меня с клавиатуры."] },
  { id: "privacy", label: "Приватность", line: ["Что мне можно видеть и что хранить.", "Что мне можно видеть и что хранить."] },
  { id: "memory", label: "Память", line: ["Что я о тебе помню. Любую запись можно стереть.", "Что я о вас помню. Любую запись можно стереть."] },
  { id: "diagnostics", label: "Состояние", line: ["Как я себя чувствую — в честных цифрах.", "Как я себя чувствую — в честных цифрах."] },
];

const STYLES: { id: Settings["persona"]["style"]; label: string; sample: [string, string] }[] = [
  { id: "witty", label: "С юмором", sample: ["Громкость тридцать. Соседи пока не жаловались.", "Громкость тридцать. Соседи пока не жаловались."] },
  { id: "warm", label: "Тёплая", sample: ["Готово, громкость тридцать. Как ты сегодня — не устал?", "Готово, громкость тридцать. Как вы сегодня — не устали?"] },
  { id: "concise", label: "Деловая", sample: ["Громкость тридцать процентов.", "Громкость тридцать процентов."] },
];

const mb = (bytes: number) => `${Math.round(bytes / 1_000_000)} МБ`;

// ---------- the face ----------

/** Eyes follow the pointer anywhere in the window. */
function useGaze(anchor: React.RefObject<HTMLElement | null>) {
  const lookX = useSpring(0, { stiffness: 160, damping: 22 });
  const [lookY, setLookY] = useState(0);
  useEffect(() => {
    let raf = 0;
    const onMove = (e: PointerEvent) => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const r = anchor.current?.getBoundingClientRect();
        if (!r) return;
        const cx = r.left + r.width / 2;
        const cy = r.top + r.height / 2;
        lookX.set(Math.max(-1, Math.min(1, (e.clientX - cx) / 420)));
        setLookY(Math.round(Math.max(-2, Math.min(3, (e.clientY - cy) / 120))));
      });
    };
    window.addEventListener("pointermove", onMove);
    return () => {
      window.removeEventListener("pointermove", onMove);
      cancelAnimationFrame(raf);
    };
  }, [anchor, lookX]);
  return { lookX, lookY };
}

function Face({ mood, level, caption }: { mood: EyeMood; level: MotionValue<number>; caption: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const { lookX, lookY } = useGaze(ref);
  return (
    <div className="face" ref={ref}>
      <div className="face-notch">
        <Eyes mood={mood} level={level} lookX={mood === "speaking" ? undefined : lookX} lookY={mood === "speaking" ? 0 : lookY} scale={1.9} />
      </div>
      <div className="face-caption">{caption}</div>
    </div>
  );
}

/** Plays a voice sample and turns its real loudness into a motion value. */
function useVoicePreview() {
  const level = useMotionValue(0);
  const [playing, setPlaying] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ctx = useRef<AudioContext | null>(null);
  const audio = useRef<HTMLAudioElement | null>(null);

  const stop = useCallback(() => {
    audio.current?.pause();
    audio.current = null;
    level.set(0);
    setPlaying(null);
  }, [level]);

  const play = useCallback(
    async (key: string, voice: string, rate: number, text?: string) => {
      stop();
      setError(null);
      setPlaying(key);
      try {
        const url = await invoke<string>("tts_preview", { voice, rate, text: text ?? null });
        const el = new Audio(url);
        audio.current = el;
        ctx.current ??= new AudioContext();
        const src = ctx.current.createMediaElementSource(el);
        const analyser = ctx.current.createAnalyser();
        analyser.fftSize = 1024;
        src.connect(analyser);
        analyser.connect(ctx.current.destination);
        const buf = new Float32Array(analyser.fftSize);
        const tick = () => {
          if (audio.current !== el) return;
          analyser.getFloatTimeDomainData(buf);
          let sum = 0;
          for (const v of buf) sum += v * v;
          const rms = Math.sqrt(sum / buf.length);
          const db = 20 * Math.log10(Math.max(rms, 1e-5));
          level.set(Math.max(0, Math.min(1, (db + 55) / 45)));
          requestAnimationFrame(tick);
        };
        el.onended = () => audio.current === el && stop();
        await el.play();
        requestAnimationFrame(tick);
      } catch (e) {
        setError(errorMessage(e));
        setPlaying(null);
      }
    },
    [level, stop],
  );

  return { level, playing, error, play, stop };
}

// ---------- building blocks ----------

function Row({ label, hint, children, wide }: { label: ReactNode; hint?: ReactNode; children: ReactNode; wide?: boolean }) {
  return (
    <div className={`row${wide ? " is-wide" : ""}`}>
      <div className="row-text">
        <div className="row-label">{label}</div>
        {hint && <div className="row-hint">{hint}</div>}
      </div>
      <div className="row-control">{children}</div>
    </div>
  );
}

function Group({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <div className="group">
      {title && <h2>{title}</h2>}
      <div className="group-rows">{children}</div>
    </div>
  );
}

function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button role="switch" aria-checked={checked} aria-label={label} className={`switch${checked ? " is-on" : ""}`} onClick={() => onChange(!checked)}>
      <span />
    </button>
  );
}

function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { id: T; label: string }[]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button key={o.id} role="radio" aria-checked={value === o.id} className={value === o.id ? "is-on" : ""} onClick={() => onChange(o.id)}>
          {value === o.id && <motion.span layoutId={`seg-${label}`} className="segmented-thumb" transition={{ type: "spring", stiffness: 500, damping: 38 }} />}
          <span className="segmented-text">{o.label}</span>
        </button>
      ))}
    </div>
  );
}

function Keycaps({ value }: { value: string }) {
  const keys = value
    .split("+")
    .map((k) => k.trim())
    .filter(Boolean);
  if (keys.length === 0) return null;
  return (
    <span className="keycaps" aria-hidden="true">
      {keys.map((k, i) => (
        <kbd key={`${k}-${i}`}>{k}</kbd>
      ))}
    </span>
  );
}

function KeyField({ kind, has, onChanged, baseUrl, notify }: { kind: "llm" | "voice"; has: boolean; onChanged: () => void; baseUrl?: string; notify: (tone: Tone, text: string) => void }) {
  const [value, setValue] = useState("");
  const [editing, setEditing] = useState(!has);
  useEffect(() => setEditing(!has), [has]);

  const save = async () => {
    try {
      await invoke("secret_set", { kind, value, baseUrl: baseUrl ?? null });
      setValue("");
      notify("ok", "Ключ сохранён в диспетчере учётных данных Windows");
      onChanged();
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };
  const clear = async () => {
    await invoke("secret_clear", { kind });
    notify("ok", "Ключ удалён");
    onChanged();
  };

  return (
    <Row
      label="API-ключ"
      hint={`Хранится в диспетчере учётных данных Windows, не в файлах приложения.${kind === "llm" ? " Отправляется только на адрес, для которого сохранён." : ""}`}
    >
      {editing ? (
        <div className="inline">
          <input
            type="password"
            value={value}
            autoComplete="off"
            spellCheck={false}
            placeholder="Вставьте ключ"
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && value.trim() && void save()}
          />
          <button className="btn btn-primary" disabled={!value.trim()} onClick={() => void save()}>
            Сохранить
          </button>
          {has && (
            <button className="btn" onClick={() => setEditing(false)}>
              Отмена
            </button>
          )}
        </div>
      ) : (
        <div className="inline">
          <span className="ok-text">Ключ сохранён</span>
          <button className="btn" onClick={() => setEditing(true)}>
            Заменить
          </button>
          <button className="btn btn-danger" onClick={() => void clear()}>
            Удалить
          </button>
        </div>
      )}
    </Row>
  );
}

/** Download state of the speech models. */
function useModels() {
  const [models, setModels] = useState<SpeechModel[]>([]);
  const [progress, setProgress] = useState<ModelProgress | null>(null);
  const refresh = useCallback(() => void invoke<SpeechModel[]>("stt_models").then(setModels), []);
  useEffect(() => {
    refresh();
    const un = listen<ModelProgress>("voice:model", (e) => {
      setProgress(e.payload);
      if (e.payload.state !== "downloading") refresh();
    });
    return () => void un.then((f) => f());
  }, [refresh]);
  const download = async (id: string) => {
    try {
      setProgress({ id, done: 0, total: 1, state: "downloading", error: null });
      await invoke("stt_download", { id });
    } catch (e) {
      setProgress({ id, done: 0, total: 1, state: "error", error: errorMessage(e) });
    }
  };
  return { models, progress, download };
}

function DownloadControl({ model, progress, onDownload }: { model: SpeechModel; progress: ModelProgress | null; onDownload: () => void }) {
  const mine = progress?.id === model.id ? progress : null;
  if (model.installed) return <span className="ok-text">Скачана</span>;
  if (mine?.state === "downloading") {
    const pct = Math.min(100, (mine.done / Math.max(1, mine.total)) * 100);
    return (
      <span className="progress">
        <span className="meter">
          <i style={{ width: `${pct}%` }} />
        </span>
        <span className="progress-text">{pct >= 100 ? "Распаковка" : `${Math.floor(pct)}%`}</span>
        <button className="btn" onClick={() => void invoke("stt_download_cancel")}>
          Отмена
        </button>
      </span>
    );
  }
  return (
    <span className="inline">
      {mine?.state === "error" && <span className="error-text">{mine.error}</span>}
      <button className="btn" disabled={progress?.state === "downloading"} onClick={onDownload}>
        Скачать, {mb(model.size)}
      </button>
    </span>
  );
}

// ---------- the window ----------

export default function SettingsApp() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [section, setSection] = useState<Section>("persona");
  const [status, setStatus] = useState<{ tone: Tone; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [happy, setHappy] = useState(false);
  const [memories, setMemories] = useState<Memory[]>([]);
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [diag, setDiag] = useState<Diagnostics | null>(null);
  const [shortcutProblems, setShortcutProblems] = useState<string[]>([]);
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  const [voiceStatus, setVoiceStatus] = useState<VoiceStatus | null>(null);
  const [ttsVoices, setTtsVoices] = useState<TtsVoice[]>([]);
  const preview = useVoicePreview();
  const { models, progress, download } = useModels();
  const statusTimer = useRef<number | undefined>(undefined);

  const notify = useCallback((tone: Tone, text: string) => {
    setStatus({ tone, text });
    window.clearTimeout(statusTimer.current);
    if (tone !== "info") statusTimer.current = window.setTimeout(() => setStatus(null), 5000);
  }, []);

  const load = async () => {
    const v = await invoke<SettingsView>("settings_get");
    setView(v);
    setDraft(v.settings);
  };
  const loadVoices = () => void invoke<TtsVoice[]>("tts_voices").then(setTtsVoices).catch(() => setTtsVoices([]));
  useEffect(() => {
    void load();
    loadVoices();
    void invoke<string[]>("shortcut_problems").then(setShortcutProblems);
  }, []);
  useEffect(() => {
    if (section === "memory") {
      void invoke<Memory[]>("memories_list").then(setMemories);
      void invoke<TaskRecord[]>("tasks_recent").then(setTasks);
    }
    if (section === "diagnostics") {
      void invoke<Diagnostics>("diagnostics_get").then(setDiag);
      void invoke<VoiceStatus>("voice_status").then(setVoiceStatus);
    }
    if (section === "voice") void invoke<AudioDevices>("audio_devices").then(setDevices);
  }, [section]);
  useEffect(() => {
    if (progress?.state === "done") loadVoices();
  }, [progress]);

  const dirty = useMemo(() => JSON.stringify(view?.settings) !== JSON.stringify(draft), [view, draft]);
  if (!view || !draft) return <div className="settings-loading" />;

  const vy = draft.persona.address === "vy";
  const preset = view.presets.find((p) => p.id === draft.provider.preset) ?? view.presets[0]!;
  const set = (patch: (s: Settings) => Settings) => setDraft((d) => (d ? patch(structuredClone(d)) : d));
  const current = SECTIONS.find((s) => s.id === section)!;
  const voiceName = ttsVoices.find((v) => v.id === draft.voice.ttsVoice)?.name ?? (ttsVoices[0]?.name ?? "голос Windows");
  const mood: EyeMood = preview.playing ? "speaking" : happy ? "happy" : busy ? "thinking" : "attentive";

  const save = async () => {
    try {
      await invoke("settings_save", { settings: draft });
      await load();
      setShortcutProblems(await invoke<string[]>("shortcut_problems"));
      notify("ok", "Сохранено");
      setHappy(true);
      window.setTimeout(() => setHappy(false), 1600);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const test = async () => {
    setBusy(true);
    notify("info", "Проверяю подключение…");
    try {
      if (dirty) await invoke("settings_save", { settings: draft });
      const r = await invoke<{ latencyMs: number; model: string; reply: string }>("provider_test");
      notify("ok", `Модель ${r.model} ответила «${r.reply}» за ${r.latencyMs} мс`);
      await load();
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const ttsModels = models.filter((m) => m.kind === "tts");
  const sttModels = models.filter((m) => m.kind === "stt");

  return (
    <div className="settings">
      <aside className="rail">
        <Face
          mood={mood}
          level={preview.level}
          caption={
            <>
              <strong>Джарвис</strong>
              <span>{draft.provider.model || "модель не выбрана"}</span>
              <span>{draft.voice.engine === "local" ? voiceName : "Qwen Omni Realtime"}</span>
            </>
          }
        />
        <nav className="nav" aria-label="Разделы">
          {SECTIONS.map((s) => (
            <button key={s.id} className={`nav-item${section === s.id ? " is-active" : ""}`} aria-current={section === s.id ? "page" : undefined} onClick={() => setSection(s.id)}>
              {section === s.id && <motion.span layoutId="nav-mark" className="nav-mark" transition={{ type: "spring", stiffness: 520, damping: 40 }} />}
              {s.label}
            </button>
          ))}
        </nav>
      </aside>

      <main className="page">
        <div className="page-scroll">
          <header className="page-head" key={section}>
            <h1>{current.label}</h1>
            <p className="says">{vy ? current.line[1] : current.line[0]}</p>
          </header>

          {section === "persona" && (
            <>
              <Group>
                <Row label={vy ? "Как вас зовут" : "Как тебя зовут"} hint="Я буду иногда обращаться по имени — не в каждой фразе.">
                  <input
                    value={draft.persona.userName}
                    placeholder="Имя"
                    spellCheck={false}
                    onChange={(e) => set((s) => ({ ...s, persona: { ...s.persona, userName: e.target.value } }))}
                  />
                </Row>
                <Row label="Обращение">
                  <Segmented
                    label="Обращение"
                    value={draft.persona.address}
                    options={[
                      { id: "ty", label: "на «ты»" },
                      { id: "vy", label: "на «вы»" },
                    ]}
                    onChange={(v) => set((s) => ({ ...s, persona: { ...s.persona, address: v } }))}
                  />
                </Row>
              </Group>
              <Group title="Манера разговора">
                <div className="styles" role="radiogroup" aria-label="Манера разговора">
                  {STYLES.map((st) => {
                    const on = draft.persona.style === st.id;
                    const sample = vy ? st.sample[1] : st.sample[0];
                    return (
                      <div key={st.id} className={`style${on ? " is-on" : ""}`}>
                        <button role="radio" aria-checked={on} className="style-pick" onClick={() => set((s) => ({ ...s, persona: { ...s.persona, style: st.id } }))}>
                          <span className="style-label">{st.label}</span>
                          <span className="style-sample">«{sample}»</span>
                        </button>
                        <button
                          className="play"
                          aria-label={`Послушать: ${st.label}`}
                          onClick={() => (preview.playing === st.id ? preview.stop() : void preview.play(st.id, draft.voice.ttsVoice, draft.voice.ttsRate, sample))}
                        >
                          {preview.playing === st.id ? <span className="play-stop" /> : <span className="play-tri" />}
                        </button>
                      </div>
                    );
                  })}
                </div>
                <p className="note">
                  Я разговариваю не только о задачах: можно просто поболтать о дне, фильмах, идеях. Что {vy ? "вы расскажете" : "ты расскажешь"} о себе, я запомню —
                  список в разделе «Память».
                </p>
              </Group>
            </>
          )}

          {section === "voice" && (
            <>
              <Group>
                <Row label="Голосовой режим" hint="Ctrl+Alt+K или кнопка микрофона на острове.">
                  <Switch label="Голосовой режим" checked={draft.voice.enabled} onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, enabled: v } }))} />
                </Row>
                {draft.voice.engine === "local" && (
                  <Row
                    label="Откликаться на имя"
                    hint={`«Джарвис…» или «Джар…» в начале фразы — и я слушаю, без мыши. «Стоп» или «хватит» — замолкаю и снова жду имени. Микрофон при этом остаётся включённым, Windows показывает его значок; фразы без имени разбираются на этом компьютере и сразу забываются.`}
                  >
                    <Switch label="Откликаться на имя" checked={draft.voice.wakeWord} onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, wakeWord: v } }))} />
                  </Row>
                )}
                <Row label="Где работает голос">
                  <Segmented
                    label="Где работает голос"
                    value={draft.voice.engine}
                    options={[
                      { id: "local", label: "На этом компьютере" },
                      { id: "realtime", label: "Qwen Omni Realtime" },
                    ]}
                    onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, engine: v } }))}
                  />
                </Row>
              </Group>

              {draft.voice.engine === "local" ? (
                <>
                  <Group title="Слух">
                    {sttModels.map((m) => (
                      <Row
                        key={m.id}
                        label={m.label}
                        hint="Модель Сбера, обучена на русской речи, работает на процессоре. Вместе с ней скачивается Silero VAD — он отличает речь от шума и музыки."
                      >
                        <DownloadControl model={m} progress={progress} onDownload={() => void download(m.id)} />
                      </Row>
                    ))}
                  </Group>

                  <Group title="Голос">
                    <div className="voices" role="radiogroup" aria-label="Голос Джарвиса">
                      {[{ id: "", name: "Лучший из скачанных", language: "" }, ...ttsVoices].map((v) => {
                        const on = draft.voice.ttsVoice === v.id;
                        const key = `voice:${v.id}`;
                        return (
                          <div key={v.id || "auto"} className={`voice${on ? " is-on" : ""}`}>
                            <button role="radio" aria-checked={on} className="voice-pick" onClick={() => set((s) => ({ ...s, voice: { ...s.voice, ttsVoice: v.id } }))}>
                              <span className="voice-mark" aria-hidden="true" />
                              <span className="voice-name">{v.name}</span>
                            </button>
                            <button
                              className="play"
                              aria-label={`Послушать: ${v.name}`}
                              onClick={() => (preview.playing === key ? preview.stop() : void preview.play(key, v.id, draft.voice.ttsRate))}
                            >
                              {preview.playing === key ? <span className="play-stop" /> : <span className="play-tri" />}
                            </button>
                          </div>
                        );
                      })}
                    </div>
                    {preview.error && <p className="error-text">{preview.error}</p>}
                    <Row label="Скорость речи" hint={`${draft.voice.ttsRate.toFixed(1).replace(".", ",")}×`}>
                      <input
                        className="slider"
                        type="range"
                        min={0.7}
                        max={1.6}
                        step={0.05}
                        value={draft.voice.ttsRate}
                        aria-label="Скорость речи"
                        onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, ttsRate: Number(e.target.value) } }))}
                      />
                    </Row>
                    {ttsModels.some((m) => !m.installed) && (
                      <div className="packs">
                        {ttsModels
                          .filter((m) => !m.installed)
                          .map((m) => (
                            <Row key={m.id} label={m.label} hint="Ещё голоса, работают на этом компьютере.">
                              <DownloadControl model={m} progress={progress} onDownload={() => void download(m.id)} />
                            </Row>
                          ))}
                      </div>
                    )}
                  </Group>
                </>
              ) : (
                <Group title="Qwen Omni Realtime">
                  <Row label="Адрес" hint="WebSocket-адрес Model Studio без параметра ?model=">
                    <input className="mono" value={draft.voice.realtimeUrl} spellCheck={false} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, realtimeUrl: e.target.value } }))} />
                  </Row>
                  <Row label="Модель">
                    <input className="mono" value={draft.voice.model} spellCheck={false} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, model: e.target.value } }))} />
                  </Row>
                  <Row label="Голос" hint="Для русского: Katerina, Marina, Alek, Tina.">
                    <input value={draft.voice.voice} spellCheck={false} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, voice: e.target.value } }))} />
                  </Row>
                  <Row label="Ключ модели" hint="Один аккаунт Model Studio для текста и голоса.">
                    <Switch label="Ключ модели" checked={draft.voice.reuseLlmKey} onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, reuseLlmKey: v } }))} />
                  </Row>
                  {!draft.voice.reuseLlmKey && <KeyField kind="voice" has={view.hasVoiceKey} onChanged={() => void load()} notify={notify} />}
                </Group>
              )}

              <Group title="Устройства">
                <Row label="Микрофон">
                  <select value={draft.voice.inputDevice} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, inputDevice: e.target.value } }))}>
                    <option value="">По умолчанию{devices?.defaultInput ? ` (${devices.defaultInput})` : ""}</option>
                    {devices?.inputs.map((d) => (
                      <option key={d} value={d}>
                        {d}
                      </option>
                    ))}
                  </select>
                </Row>
                <Row label="Вывод звука">
                  <select value={draft.voice.outputDevice} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, outputDevice: e.target.value } }))}>
                    <option value="">По умолчанию{devices?.defaultOutput ? ` (${devices.defaultOutput})` : ""}</option>
                    {devices?.outputs.map((d) => (
                      <option key={d} value={d}>
                        {d}
                      </option>
                    ))}
                  </select>
                </Row>
              </Group>
              <p className="note">
                Перебить меня можно голосом — просто начни говорить. «Стоп» или «хватит» останавливают и ответ, и задачу. В наушниках я слышу {vy ? "вас" : "тебя"} лучше
                всего.
              </p>
            </>
          )}

          {section === "model" && (
            <Group>
              <Row label="Провайдер">
                <select
                  value={draft.provider.preset}
                  onChange={(e) => {
                    const p = view.presets.find((x) => x.id === e.target.value)!;
                    set((s) => ({ ...s, provider: { ...s.provider, preset: p.id, baseUrl: "", model: p.defaultModel || s.provider.model } }));
                  }}
                >
                  {view.presets.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </Row>
              <Row label="Адрес API" hint={preset.baseUrl ? `Пусто — ${preset.baseUrl}` : "Укажите адрес OpenAI-совместимого API."}>
                <input
                  className="mono"
                  value={draft.provider.baseUrl}
                  placeholder={preset.baseUrl}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, baseUrl: e.target.value } }))}
                />
              </Row>
              <Row label="Модель" hint={preset.dashscope ? "qwen3.8-flash — быстрее, qwen3.8-max — сильнее." : undefined}>
                <input className="mono" value={draft.provider.model} spellCheck={false} onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, model: e.target.value } }))} />
              </Row>
              <Row label="Модель для изображений" hint="Пусто — та же модель.">
                <input
                  className="mono"
                  value={draft.provider.visionModel}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, visionModel: e.target.value } }))}
                />
              </Row>
              {preset.needsKey && (
                <KeyField kind="llm" has={view.hasLlmKey} baseUrl={draft.provider.baseUrl.trim() || preset.baseUrl} onChanged={() => void load()} notify={notify} />
              )}
              <Row label="Связь с моделью" hint="Короткий запрос: адрес, ключ и модель.">
                <button className="btn" disabled={busy} onClick={() => void test()}>
                  {busy ? "Проверяю…" : "Проверить подключение"}
                </button>
              </Row>
            </Group>
          )}

          {section === "behaviour" && (
            <Group>
              <Row label="Спрашивать перед изменениями" hint="Чтение файлов, команды, меняющие проект, закрытие окон. Отправка сообщений, удаление и покупки подтверждаются всегда.">
                <Switch label="Спрашивать перед изменениями" checked={draft.agent.confirmModerate} onChange={(v) => set((s) => ({ ...s, agent: { ...s.agent, confirmModerate: v } }))} />
              </Row>
              <Row label="Папки проектов" hint="По одной на строке. Помогают понять «мой проект».">
                <textarea
                  className="mono"
                  rows={4}
                  value={draft.agent.projectRoots.join("\n")}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, agent: { ...s.agent, projectRoots: e.target.value.split("\n").map((l) => l.trim()).filter(Boolean) } }))}
                />
              </Row>
              <Row label="Предел шагов на задачу" hint="Чтобы задача не крутилась бесконечно.">
                <input
                  className="narrow"
                  type="number"
                  min={4}
                  max={60}
                  value={draft.agent.maxSteps}
                  onChange={(e) => set((s) => ({ ...s, agent: { ...s.agent, maxSteps: Number(e.target.value) || 24 } }))}
                />
              </Row>
              <Row label="Запускать вместе с Windows">
                <Switch label="Запускать вместе с Windows" checked={draft.launchAtStartup} onChange={(v) => set((s) => ({ ...s, launchAtStartup: v }))} />
              </Row>
              <Row label="Меньше движения" hint="Мгновенные переходы вместо пружинной анимации.">
                <Switch label="Меньше движения" checked={draft.reduceMotion} onChange={(v) => set((s) => ({ ...s, reduceMotion: v }))} />
              </Row>
            </Group>
          )}

          {section === "shortcuts" && (
            <>
              <Group>
                <Row label="Открыть или свернуть чат" hint="Например, Ctrl+Alt+J. Сочетания с Win заняты системой.">
                  <div className="inline">
                    <input className="mono narrow-wide" value={draft.shortcuts.workspace} spellCheck={false} onChange={(e) => set((s) => ({ ...s, shortcuts: { ...s.shortcuts, workspace: e.target.value } }))} />
                    <Keycaps value={draft.shortcuts.workspace} />
                  </div>
                </Row>
                <Row label="Голосовой режим">
                  <div className="inline">
                    <input className="mono narrow-wide" value={draft.shortcuts.voice} spellCheck={false} onChange={(e) => set((s) => ({ ...s, shortcuts: { ...s.shortcuts, voice: e.target.value } }))} />
                    <Keycaps value={draft.shortcuts.voice} />
                  </div>
                </Row>
              </Group>
              {shortcutProblems.length > 0 && (
                <div className="callout">
                  {shortcutProblems.map((p) => (
                    <div key={p}>{p}</div>
                  ))}
                </div>
              )}
              <p className="note">Esc закрывает чат. Остановить задачу — кнопка в чате, пункт в меню трея или голосом: «стоп».</p>
            </>
          )}

          {section === "privacy" && (
            <Group>
              <Row label="Хранить историю разговоров" hint="Локально, в базе на этом компьютере. Снимки экрана и звук не сохраняются никогда.">
                <Switch label="Хранить историю разговоров" checked={draft.privacy.storeHistory} onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, storeHistory: v } }))} />
              </Row>
              <Row label="Разрешить снимки экрана" hint="Только по ходу задачи, которой это нужно. Снимок уходит выбранной модели.">
                <Switch label="Разрешить снимки экрана" checked={draft.privacy.allowScreen} onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, allowScreen: v } }))} />
              </Row>
              <Row label="Читать буфер обмена без вопроса">
                <Switch
                  label="Читать буфер обмена без вопроса"
                  checked={draft.privacy.allowClipboardRead}
                  onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, allowClipboardRead: v } }))}
                />
              </Row>
            </Group>
          )}

          {section === "memory" && (
            <>
              <Group title="Что я помню">
                {memories.length === 0 ? (
                  <p className="empty">Пока ничего. {vy ? "Скажите" : "Скажи"} «запомни, что…» — или просто {vy ? "расскажите" : "расскажи"} о себе.</p>
                ) : (
                  <ul className="memories">
                    {memories.map((m) => (
                      <li key={m.id}>
                        <span>{m.text}</span>
                        <button className="btn btn-danger" onClick={() => void invoke("memory_delete", { id: m.id }).then(() => invoke<Memory[]>("memories_list").then(setMemories))}>
                          Забыть
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </Group>
              <Group title="Последние задачи">
                {tasks.length === 0 ? (
                  <p className="empty">Задач ещё не было.</p>
                ) : (
                  <ul className="tasks">
                    {tasks.map((t) => (
                      <li key={t.id}>
                        <span className={`dot dot-${t.status}`} title={t.status} />
                        <span className="task-text">{t.request}</span>
                        <span className="task-time">
                          {new Date(t.startedAt).toLocaleString("ru-RU", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" })}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
                <div className="actions">
                  <button className="btn btn-danger" onClick={() => void invoke("history_clear").then(() => (setTasks([]), notify("ok", "История очищена")))}>
                    Очистить историю
                  </button>
                </div>
              </Group>
            </>
          )}

          {section === "diagnostics" &&
            (diag ? (
              <Group>
                <Row label="Время работы">
                  <span className="value">{Math.round(diag.uptimeSecs / 60)} мин</span>
                </Row>
                <Row label="Память процесса" hint="Без WebView2.">
                  <span className="value">{diag.workingSetMb.toFixed(1).replace(".", ",")} МБ</span>
                </Row>
                <Row label="Трекер курсора">
                  <span className="value">{diag.trackerHz.toFixed(1).replace(".", ",")} Гц</span>
                </Row>
                <Row label="Мониторы">
                  <span className="value">
                    {diag.monitors.map((m) => `${m.bounds.width}×${m.bounds.height}, ${Math.round(m.scale * 100)}%${m.primary ? ", основной" : ""}`).join("; ")}
                  </span>
                </Row>
                <Row label="Распознавание последней фразы">
                  <span className="value">{voiceStatus?.metrics.lastSttMs != null ? `${voiceStatus.metrics.lastSttMs} мс` : "ещё не было"}</span>
                </Row>
                <Row label="От конца фразы до моего ответа">
                  <span className="value">{voiceStatus?.metrics.lastTimeToFirstAudioMs != null ? `${voiceStatus.metrics.lastTimeToFirstAudioMs} мс` : "ещё не было"}</span>
                </Row>
                <Row label="Журнал" wide>
                  <span className="value mono path">{view.logPath}</span>
                </Row>
                <Row label="Данные" wide>
                  <span className="value mono path">{view.dataPath}</span>
                </Row>
              </Group>
            ) : (
              <p className="empty">Измеряю…</p>
            ))}
        </div>

        <AnimatePresence>
          {(dirty || status) && (
            <motion.footer
              className="savebar"
              initial={{ y: 24, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              exit={{ y: 24, opacity: 0 }}
              transition={{ type: "spring", stiffness: 420, damping: 34 }}
            >
              <span className={`savebar-status ${status?.tone ?? ""}`} role="status">
                {status?.text ?? "Есть несохранённые изменения"}
              </span>
              {dirty && (
                <>
                  <button className="btn" onClick={() => setDraft(view.settings)}>
                    Отменить
                  </button>
                  <button className="btn btn-primary" onClick={() => void save()}>
                    Сохранить
                  </button>
                </>
              )}
            </motion.footer>
          )}
        </AnimatePresence>
      </main>
    </div>
  );
}
