// Settings. The face from the island lives in the left rail — its eyes follow
// the pointer, become real audio bars while a voice sample plays, and smile
// when changes are saved. Eight short sections, plain words, only the
// settings people actually change. Secrets go straight to the system key
// store through `secret_set`; the UI only ever learns whether a key exists.

import { AnimatePresence, motion, useMotionValue, useSpring, type MotionValue } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { settingsPlatformCopy } from "../lib/platform";
import {
  CURATED_EDGE_VOICES,
  ensureWorkspaceFeeds,
  hasTauri,
  useWorkspaceRoutines,
  useWorkspaceTts,
  workspaceIpc,
  type RoutineItem,
} from "../lib/workspace-ipc";
import { Eyes, type EyeMood } from "../island/Eyes";
import "./settings.css";
import { TitleBar } from "./TitleBar";
import { Group, Row, Segmented, Switch } from "./controls";
import { McpSection } from "./McpSection";
import { ACTIVITIES, EMOTES, emote, useEmote, type Activity } from "../store/emote";
import { LiveAudioBars, routineIconName, WsIcon } from "../workspace/WorkspaceIcons";
import { Icon, type IconName } from "../ui/Icon";
import { startUpdatesFeed, useUpdates } from "../store/updates";

const platformCopy = settingsPlatformCopy();

interface Preset {
  id: string;
  label: string;
  baseUrl: string;
  defaultModel: string;
  needsKey: boolean;
  dashscope: boolean;
}

export interface Settings {
  provider: { preset: string; baseUrl: string; model: string; visionModel: string; temperature: number };
  voice: {
    enabled: boolean;
    engine: "local" | "realtime";
    sttModel: string;
    ttsVoice: string;
    ttsRate: number;
    ttsPitch: number;
    autoSpeakReplies: boolean;
    wakeWord: boolean;
    realtimeUrl: string;
    model: string;
    voice: string;
    reuseLlmKey: boolean;
    inputDevice: string;
    outputDevice: string;
    duckMedia: boolean;
  };
  persona: { userName: string; address: "ty" | "vy"; style: "witty" | "warm" | "concise" };
  shortcuts: { workspace: string; voice: string; clipboard: string };
  privacy: { storeHistory: boolean; allowScreen: boolean; allowClipboardRead: boolean };
  clipboard: { enabled: boolean; maxItems: number; keepDays: number; skipSecrets: boolean; ignoredApps: string[] };
  agent: { maxSteps: number; confirmModerate: boolean; fullControl: boolean; projectRoots: string[] };
  launchAtStartup: boolean;
  reduceMotion: boolean;
  updates: { autoCheck: boolean };
}

interface SettingsView {
  settings: Settings;
  presets: Preset[];
  hasLlmKey: boolean;
  hasVoiceKey: boolean;
  hasAzureKey: boolean;
  hasElevenlabsKey: boolean;
  hasYandexKey: boolean;
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

export type Section = "general" | "model" | "voice" | "persona" | "behaviour" | "privacy" | "memory" | "mcp";
type Tone = "ok" | "error" | "info";

const FALLBACK_SETTINGS_VIEW: SettingsView = {
  settings: {
    provider: {
      preset: "dashscope",
      baseUrl: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
      model: "qwen3.8-max",
      visionModel: "",
      temperature: 0.3,
    },
    voice: {
      enabled: true,
      engine: "local",
      sttModel: "gigaam-v2-rnnt",
      ttsVoice: "edge:ru-RU-DmitryNeural",
      ttsRate: 1.0,
      ttsPitch: 0,
      autoSpeakReplies: true,
      wakeWord: true,
      realtimeUrl: "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime",
      model: "qwen-omni-turbo-realtime",
      voice: "Alek",
      reuseLlmKey: true,
      inputDevice: "",
      outputDevice: "",
      duckMedia: true,
    },
    persona: { userName: "Александр", address: "ty", style: "witty" },
    shortcuts: { workspace: "Ctrl+Alt+J", voice: "Ctrl+Alt+K", clipboard: "Ctrl+Alt+V" },
    privacy: { storeHistory: true, allowScreen: true, allowClipboardRead: true },
    clipboard: { enabled: true, maxItems: 200, keepDays: 30, skipSecrets: true, ignoredApps: ["KeepassXC.exe"] },
    agent: {
      maxSteps: 28,
      confirmModerate: false,
      fullControl: true,
      projectRoots: ["C:\\Users\\-pin\\Documents\\Jarvis"],
    },
    launchAtStartup: true,
    reduceMotion: false,
    updates: { autoCheck: true },
  },
  presets: [
    {
      id: "dashscope",
      label: "Alibaba Model Studio (Qwen)",
      baseUrl: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
      defaultModel: "qwen3.8-max",
      needsKey: true,
      dashscope: true,
    },
    {
      id: "deepseek",
      label: "DeepSeek",
      baseUrl: "https://api.deepseek.com/v1",
      defaultModel: "deepseek-chat",
      needsKey: true,
      dashscope: false,
    },
  ],
  hasLlmKey: true,
  hasVoiceKey: true,
  hasAzureKey: false,
  hasElevenlabsKey: false,
  hasYandexKey: false,
  logPath: "C:\\Users\\-pin\\AppData\\Local\\dev.jarvis.desktop\\logs\\jarvis.log",
  dataPath: "C:\\Users\\-pin\\AppData\\Local\\dev.jarvis.desktop\\jarvis.db",
};

function normalizeSettingsView(v: SettingsView): SettingsView {
  return {
    ...v,
    settings: {
      ...v.settings,
      updates: v.settings.updates ?? { autoCheck: true },
      voice: {
        ...v.settings.voice,
        // The realtime engine left the settings (2026-11-23): one voice pipeline.
        engine: "local",
        ttsVoice: v.settings.voice.ttsVoice || "edge:ru-RU-DmitryNeural",
        ttsRate: typeof v.settings.voice.ttsRate === "number" ? v.settings.voice.ttsRate : 1.0,
        ttsPitch: typeof v.settings.voice.ttsPitch === "number" ? v.settings.voice.ttsPitch : 0,
        autoSpeakReplies: !!v.settings.voice.autoSpeakReplies,
      },
    },
  };
}

/** Sections, with one plain line under each title. */
const SECTIONS: { id: Section; label: string; icon: IconName; hint: string }[] = [
  { id: "general", label: "Основное", icon: "settings", hint: "Запуск, обновления и сочетания клавиш." },
  { id: "model", label: "Модель", icon: "bolt", hint: "Нейросеть, которая понимает просьбы и выполняет задачи." },
  { id: "voice", label: "Голос", icon: "mic", hint: "Как Джарвис говорит и как слышит." },
  { id: "persona", label: "Характер", icon: "chat", hint: "Обращение и манера разговора." },
  { id: "behaviour", label: "Самостоятельность", icon: "target", hint: "Что Джарвис делает сам, а что — только с вашего разрешения." },
  { id: "privacy", label: "Приватность", icon: "eye", hint: "Что можно видеть и что хранить на этом компьютере." },
  { id: "memory", label: "Память", icon: "clipboard", hint: "Что Джарвис помнит о вас. Любую запись можно стереть." },
  { id: "mcp", label: "Подключения", icon: "globe", hint: "Инструменты других программ по протоколу MCP." },
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

function Face({
  mood,
  level,
  caption,
  activity = null,
}: {
  mood: EyeMood;
  level: MotionValue<number>;
  caption: ReactNode;
  activity?: Activity | null;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const { lookX, lookY } = useGaze(ref);
  const { emote: current, seq } = useEmote();
  const still = mood === "speaking" || !!activity;
  return (
    <div className="face" ref={ref}>
      <div className="face-notch">
        <Eyes
          mood={mood}
          level={level}
          lookX={still ? undefined : lookX}
          lookY={still ? 0 : lookY}
          scale={1.9}
          emote={current}
          emoteSeq={seq}
          activity={activity}
          onBoop={() => emote("boop")}
        />
      </div>
      <div className="face-caption">{caption}</div>
    </div>
  );
}

/** Plays a voice sample via native `tts_speak` (or `tts_preview`) and turns real loudness into a motion value. */
function useVoicePreview() {
  const level = useMotionValue(0);
  const [playing, setPlaying] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ctx = useRef<AudioContext | null>(null);
  const audio = useRef<HTMLAudioElement | null>(null);
  const ttsSpeaking = useWorkspaceTts((s) => s.speaking);
  const ttsLevel = useWorkspaceTts((s) => s.level);

  useEffect(() => {
    if (ttsSpeaking) {
      level.set(Math.max(0.18, ttsLevel));
    } else if (!audio.current) {
      level.set(0);
      setPlaying((prev) => (prev?.startsWith("tts:") ? null : prev));
    }
  }, [ttsSpeaking, ttsLevel, level]);

  const stop = useCallback(() => {
    audio.current?.pause();
    audio.current = null;
    level.set(0);
    setPlaying(null);
    void workspaceIpc.tts.stop();
  }, [level]);

  const play = useCallback(
    async (key: string, voice: string, rate: number, text?: string) => {
      stop();
      setError(null);
      setPlaying(`tts:${key}`);
      const phrase =
        text?.trim() || "Доброе утро. Все системы Джарвиса работают штатно, задержка синтеза минимальна.";
      try {
        await workspaceIpc.tts.speak(phrase, {
          voice: voice || undefined,
          rate,
          interrupt: true,
        });
      } catch (e) {
        // Fallback to legacy tts_preview if needed
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
        } catch (err) {
          setError(errorMessage(err ?? e));
          setPlaying(null);
        }
      }
    },
    [level, stop],
  );

  return { level, playing, error, play, stop };
}

// ---------- building blocks ----------

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

/** Yandex SpeechKit through the user's own Yandex Cloud: native Russian voices, streamed. */
function YandexVoice({
  has,
  notify,
  onChanged,
  onUse,
}: {
  has: boolean;
  notify: (tone: Tone, text: string) => void;
  onChanged: () => void;
  onUse: (id: string) => void;
}) {
  return (
    <>
      <p className="note">
        Яндекс SpeechKit: родные русские голоса с правильными ударениями, около 1 ₸ за фразу. Нужен API-ключ сервисного аккаунта с ролью ai.speechkit-tts.user. Наружу уходит только текст ответа; без интернета говорит локальный голос.
      </p>
      <KeyField
        kind="yandex"
        has={has}
        notify={notify}
        onChanged={() => {
          onChanged();
          onUse("yandex:kirill");
        }}
      />
    </>
  );
}

/** ElevenLabs through the user's own plan: the most natural voice, streamed. */
function ElevenVoice({
  has,
  notify,
  onChanged,
  onUse,
}: {
  has: boolean;
  notify: (tone: Tone, text: string) => void;
  onChanged: () => void;
  onUse: (id: string) => void;
}) {
  return (
    <>
      <p className="note">
        Опциональный коммерческий облачный движок ElevenLabs v4 Turbo. Наружу уходит только текст ответа. Без интернета или когда кредиты кончились, автоматически включается бесплатный Edge Neural HD или локальный голос.
      </p>
      <KeyField
        kind="elevenlabs"
        has={has}
        notify={notify}
        onChanged={() => {
          onChanged();
          onUse("eleven:rQOBu7YxCDxGiFdTm28w");
        }}
      />
    </>
  );
}

/** Microsoft neural voices through the user's own free Azure Speech (F0). */
function AzureVoice({
  has,
  notify,
  onChanged,
  onUse,
}: {
  has: boolean;
  notify: (tone: Tone, text: string) => void;
  onChanged: () => void;
  onUse: (id: string) => void;
}) {
  const [region, setRegion] = useState("westeurope");
  return (
    <>
      <p className="note">
        Дополнительное подключение через личный ключ Azure Speech (тариф Free F0 — 500 000 символов в месяц). Не требуется для работы встроенных голосов Edge Neural HD.
      </p>
      {!has && (
        <Row label="Регион" hint="Как в Azure, латиницей: westeurope, eastus, swedencentral…">
          <input className="mono" value={region} spellCheck={false} onChange={(e) => setRegion(e.target.value.trim().toLowerCase())} />
        </Row>
      )}
      <KeyField
        kind="azure"
        has={has}
        baseUrl={region}
        notify={notify}
        onChanged={() => {
          onChanged();
          onUse("azure:ru-RU-DmitryNeural");
        }}
      />
    </>
  );
}

function KeyField({
  kind,
  has,
  onChanged,
  baseUrl,
  notify,
}: {
  kind: "llm" | "voice" | "azure" | "elevenlabs" | "yandex";
  has: boolean;
  onChanged: () => void;
  baseUrl?: string;
  notify: (tone: Tone, text: string) => void;
}) {
  const [value, setValue] = useState("");
  const [editing, setEditing] = useState(!has);
  useEffect(() => setEditing(!has), [has]);

  const save = async () => {
    try {
      await invoke("secret_set", { kind, value, baseUrl: baseUrl ?? null });
      setValue("");
      notify("ok", `Ключ сохранён в ${platformCopy.keyStoreLocation}`);
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
      hint={`Хранится в ${platformCopy.keyStoreLocation}, не в файлах приложения.${kind === "llm" ? " Отправляется только на адрес, для которого сохранён." : ""}`}
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
  const refresh = useCallback(() => {
    if (!hasTauri()) return;
    void invoke<SpeechModel[]>("stt_models").then(setModels).catch(() => {});
  }, []);
  useEffect(() => {
    refresh();
    if (!hasTauri()) return;
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

function DownloadControl({
  model,
  progress,
  onDownload,
}: {
  model: SpeechModel;
  progress: ModelProgress | null;
  onDownload: () => void;
}) {
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

// ---------- Routines Manager in Settings (Агент и управление) ----------

function RoutinesSettingsManager({ notify }: { notify: (tone: Tone, text: string) => void }) {
  const routines = useWorkspaceRoutines((s) => s.routines);
  const loadRoutines = useWorkspaceRoutines((s) => s.load);
  const saveRoutine = useWorkspaceRoutines((s) => s.save);
  const removeRoutine = useWorkspaceRoutines((s) => s.remove);
  const runRoutine = useWorkspaceRoutines((s) => s.run);

  const [editing, setEditing] = useState<{
    id: string | null;
    title: string;
    subtitle: string;
    icon: string;
    prompt: string;
    autonomy: boolean;
  } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void loadRoutines();
  }, [loadRoutines]);

  const startNew = () =>
    setEditing({
      id: null,
      title: "",
      subtitle: "",
      icon: "bolt",
      prompt: "",
      autonomy: true,
    });

  const startEdit = (r: RoutineItem) =>
    setEditing({
      id: r.id,
      title: r.title,
      subtitle: r.subtitle,
      icon: r.icon,
      prompt: r.prompt,
      autonomy: r.autonomy,
    });

  const handleSave = async () => {
    if (!editing || !editing.title.trim() || !editing.prompt.trim()) return;
    setBusy(true);
    try {
      await saveRoutine({
        id: editing.id,
        title: editing.title.trim(),
        subtitle: editing.subtitle.trim() || "Сценарий",
        icon: editing.icon,
        prompt: editing.prompt.trim(),
        autonomy: editing.autonomy,
      });
      setEditing(null);
      notify("ok", "Сценарий сохранён");
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const handleRemove = async (id: string, title: string) => {
    try {
      await removeRoutine(id);
      notify("ok", `Сценарий «${title}» удалён`);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const handleRun = async (r: RoutineItem) => {
    try {
      await runRoutine(r.id);
      notify("ok", `Запущен сценарий «${r.title}»`);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  return (
    <Group title="Сценарии">
      <p className="note" style={{ marginTop: 10, marginBottom: 14 }}>
        Готовые многошаговые поручения, доступные на стартовом экране чата и в нижней панели быстрого запуска.
      </p>

      <div className="settings-routines-list">
        {routines.map((r) => (
          <div key={r.id} className="settings-routine-row">
            <span className="settings-routine-icon">
              <WsIcon name={routineIconName(r.icon)} size={16} />
            </span>
            <div className="settings-routine-body">
              <div className="settings-routine-head">
                <span className="settings-routine-title">{r.title}</span>
                {r.autonomy && (
                  <span className="settings-routine-badge">
                    <WsIcon name="bolt" size={10} />
                    Автопилот
                  </span>
                )}
                <span className="settings-routine-sub">{r.subtitle}</span>
              </div>
              <div className="settings-routine-prompt mono">{r.prompt}</div>
            </div>
            <div className="settings-routine-actions">
              <button type="button" className="btn" onClick={() => void handleRun(r)} title="Запустить сейчас">
                <WsIcon name="play" size={11} />
                <span>Запустить</span>
              </button>
              <button type="button" className="btn" onClick={() => startEdit(r)} title="Изменить">
                Изменить
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => void handleRemove(r.id, r.title)}
                title="Удалить"
              >
                Удалить
              </button>
            </div>
          </div>
        ))}
      </div>

      {editing ? (
        <div className="settings-routine-editor">
          <div className="settings-routine-editor-title">
            {editing.id ? `Редактирование сценария «${editing.title}»` : "Новый сценарий автопилота"}
          </div>
          <Row label="Название">
            <input
              value={editing.title}
              placeholder="Например, Глубокая работа"
              onChange={(e) => setEditing({ ...editing, title: e.target.value })}
            />
          </Row>
          <Row label="Краткое описание">
            <input
              value={editing.subtitle}
              placeholder="Фокус 45 мин · чистый стол · код + браузер"
              onChange={(e) => setEditing({ ...editing, subtitle: e.target.value })}
            />
          </Row>
          <Row label="Значок">
            <Segmented
              label="Значок сценария"
              value={editing.icon as "target" | "sun" | "window" | "cpu" | "bolt"}
              options={[
                { id: "target", label: "Фокус" },
                { id: "sun", label: "Брифинг" },
                { id: "window", label: "Окна" },
                { id: "cpu", label: "ПК" },
                { id: "bolt", label: "Авто" },
              ]}
              onChange={(v) => setEditing({ ...editing, icon: v })}
            />
          </Row>
          <Row label="Инструкция для агента" hint="Какие инструменты вызвать и какой результат получить.">
            <textarea
              rows={3}
              value={editing.prompt}
              placeholder="Сверни лишние окна и включи фокус на 45 минут…"
              onChange={(e) => setEditing({ ...editing, prompt: e.target.value })}
            />
          </Row>
          <Row label="Выполнять в режиме Автопилота" hint="Разрешить умеренные действия с окнами и таймером без лишних вопросов.">
            <Switch
              label="Выполнять в режиме Автопилота"
              checked={editing.autonomy}
              onChange={(v) => setEditing({ ...editing, autonomy: v })}
            />
          </Row>
          <div className="actions" style={{ gap: 8 }}>
            <button type="button" className="btn" disabled={busy} onClick={() => setEditing(null)}>
              Отмена
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={busy || !editing.title.trim() || !editing.prompt.trim()}
              onClick={() => void handleSave()}
            >
              {busy ? "Сохраняю…" : "Сохранить сценарий"}
            </button>
          </div>
        </div>
      ) : (
        <div className="actions">
          <button type="button" className="btn btn-primary" onClick={startNew}>
            <WsIcon name="plus" size={13} />
            <span>Добавить сценарий</span>
          </button>
        </div>
      )}
    </Group>
  );
}

// ---------- the window ----------

export default function SettingsApp({
  initialSection = "general",
}: {
  initialSection?: Section;
} = {}) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [section, setSection] = useState<Section>(initialSection);
  const [status, setStatus] = useState<{ tone: Tone; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [happy, setHappy] = useState(false);
  const [memories, setMemories] = useState<Memory[]>([]);
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [shortcutProblems, setShortcutProblems] = useState<string[]>([]);
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  const [ttsVoices, setTtsVoices] = useState<TtsVoice[]>([]);

  const [samplePhrase, setSamplePhrase] = useState("Доброе утро. Всё готово: почта разобрана, на сегодня три встречи.");
  const updates = useUpdates();
  const ttsSpeaking = useWorkspaceTts((s) => s.speaking);

  const preview = useVoicePreview();
  const { models, progress, download } = useModels();
  const statusTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    setSection(initialSection);
  }, [initialSection]);

  const notify = useCallback((tone: Tone, text: string) => {
    setStatus({ tone, text });
    window.clearTimeout(statusTimer.current);
    if (tone !== "info") statusTimer.current = window.setTimeout(() => setStatus(null), 5000);
  }, []);

  const load = async () => {
    if (!hasTauri()) {
      const norm = normalizeSettingsView(FALLBACK_SETTINGS_VIEW);
      setView(norm);
      setDraft(structuredClone(norm.settings));
      return;
    }
    try {
      const v = await invoke<SettingsView>("settings_get");
      const norm = normalizeSettingsView(v ?? FALLBACK_SETTINGS_VIEW);
      setView(norm);
      setDraft(structuredClone(norm.settings));
    } catch {
      const norm = normalizeSettingsView(FALLBACK_SETTINGS_VIEW);
      setView(norm);
      setDraft(structuredClone(norm.settings));
    }
  };

  const loadVoices = () => {
    if (!hasTauri()) {
      setTtsVoices(CURATED_EDGE_VOICES.map((v) => ({ id: v.id, name: v.name, language: v.lang })));
      return;
    }
    void invoke<TtsVoice[]>("tts_voices")
      .then((list) => {
        if (Array.isArray(list) && list.length > 0) setTtsVoices(list);
        else setTtsVoices(CURATED_EDGE_VOICES.map((v) => ({ id: v.id, name: v.name, language: v.lang })));
      })
      .catch(() => setTtsVoices(CURATED_EDGE_VOICES.map((v) => ({ id: v.id, name: v.name, language: v.lang }))));
  };

  useEffect(() => {
    void ensureWorkspaceFeeds();
    void startUpdatesFeed();
    void load();
    loadVoices();
    if (hasTauri()) {
      void invoke<string[]>("shortcut_problems").then(setShortcutProblems).catch(() => {});
    }
  }, []);

  useEffect(() => {
    if (!hasTauri()) return;
    if (section === "memory") {
      void invoke<Memory[]>("memories_list").then(setMemories).catch(() => {});
      void invoke<TaskRecord[]>("tasks_recent").then(setTasks).catch(() => {});
    }
    if (section === "voice") {
      void invoke<AudioDevices>("audio_devices").then(setDevices).catch(() => {});
    }
  }, [section]);

  useEffect(() => {
    if (progress?.state === "done") loadVoices();
  }, [progress]);

  const [demoMood, setDemoMood] = useState<EyeMood | null>(null);
  const [demoActivity, setDemoActivity] = useState<Activity | null>(null);
  const demoTimer = useRef<number | undefined>(undefined);
  const dirty = useMemo(() => JSON.stringify(view?.settings) !== JSON.stringify(draft), [view, draft]);

  if (!view || !draft) return <div className="settings-loading" />;

  const vy = draft.persona.address === "vy";
  const preset = view.presets.find((p) => p.id === draft.provider.preset) ?? view.presets[0]!;
  const set = (patch: (s: Settings) => Settings) => setDraft((d) => (d ? patch(structuredClone(d)) : d));
  const current = SECTIONS.find((s) => s.id === section)!;

  const activeCurated = CURATED_EDGE_VOICES.find((v) => v.id === draft.voice.ttsVoice);
  const voiceName =
    activeCurated?.shortName ??
    ttsVoices.find((v) => v.id === draft.voice.ttsVoice)?.name ??
    "Дмитрий HD · Neural";

  const mood: EyeMood =
    demoMood ?? (preview.playing || ttsSpeaking ? "speaking" : happy ? "happy" : busy || updates.checking ? "thinking" : "attentive");

  const showMood = (m: EyeMood) => {
    window.clearTimeout(demoTimer.current);
    setDemoActivity(null);
    setDemoMood(m);
    demoTimer.current = window.setTimeout(() => setDemoMood(null), 2600);
  };
  const showActivity = (a: Activity) => {
    window.clearTimeout(demoTimer.current);
    setDemoMood("thinking");
    setDemoActivity(a);
    demoTimer.current = window.setTimeout(() => {
      setDemoMood(null);
      setDemoActivity(null);
    }, 3200);
  };

  const save = async () => {
    try {
      if (hasTauri()) {
        await invoke("settings_save", { settings: draft });
        await load();
        setShortcutProblems(await invoke<string[]>("shortcut_problems").catch(() => []));
      } else {
        setView({ ...view, settings: structuredClone(draft) });
      }
      notify("ok", "Сохранено");
      setHappy(true);
      window.setTimeout(() => setHappy(false), 1600);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  // A cloud key was just saved: make its voice the current one and store it
  // right away — reloading the settings after the key used to drop the pick.
  const useCloudVoice = async (id: string) => {
    if (!draft) return;
    const next = structuredClone(draft);
    next.voice.ttsVoice = id;
    try {
      if (hasTauri()) {
        await invoke("settings_save", { settings: next });
        await load();
      } else {
        setDraft(next);
      }
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const test = async () => {
    setBusy(true);
    notify("info", "Проверяю подключение…");
    try {
      if (dirty && hasTauri()) await invoke("settings_save", { settings: draft });
      const r = await invoke<{ latencyMs: number; model: string; reply: string }>("provider_test");
      notify("ok", `Модель ${r.model} ответила «${r.reply}» за ${r.latencyMs} мс`);
      await load();
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleFullControlLive = async (enabled: boolean) => {
    set((s) => ({ ...s, agent: { ...s.agent, fullControl: enabled } }));
    try {
      await workspaceIpc.agent.setFullControl(enabled);
      notify("ok", enabled ? "Автопилот включён" : "Автопилот выключен");
    } catch {
      /* saved via draft as well */
    }
  };

  const ttsModels = models.filter((m) => m.kind === "tts");
  const sttModels = models.filter((m) => m.kind === "stt");
  const nonEdgeVoices = ttsVoices.filter((v) => !v.id.startsWith("edge:"));

  return (
    <div className="settings">
      <TitleBar />
      <aside className="rail">
        <Face
          mood={mood}
          activity={demoActivity}
          level={preview.level}
          caption={
            <>
              <strong>Джарвис</strong>
              <span>{updates.status ? `версия ${updates.status.current}` : voiceName}</span>
            </>
          }
        />
        <nav className="nav" aria-label="Разделы">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              className={`nav-item${section === s.id ? " is-active" : ""}`}
              aria-current={section === s.id ? "page" : undefined}
              onClick={() => setSection(s.id)}
            >
              {section === s.id && (
                <motion.span
                  layoutId="nav-mark"
                  className="nav-mark"
                  transition={{ type: "spring", stiffness: 520, damping: 40 }}
                />
              )}
              <Icon name={s.icon} size={16} className="nav-icon" />
              <span>{s.label}</span>
            </button>
          ))}
        </nav>
      </aside>

      <main className="page">
        <div className="page-scroll">
          <header className="page-head" key={section}>
            <h1>{current.label}</h1>
            <p className="says">{current.hint}</p>
          </header>

          {section === "persona" && (
            <>
              <Group title="Мимика">
                <p className="note">Нажмите — лицо слева покажет выражение. Двойной клик по глазам — «тык».</p>
                <div className="expr-grid">
                  {EMOTES.map((e) => (
                    <button key={e.id} className="expr-chip" onClick={() => emote(e.id)}>
                      {e.label}
                    </button>
                  ))}
                </div>
                <div className="expr-caption">Пока работает</div>
                <div className="expr-grid">
                  {ACTIVITIES.map((a) => (
                    <button key={a.id} className="expr-chip" onClick={() => showActivity(a.id)}>
                      {a.label}
                    </button>
                  ))}
                </div>
                <div className="expr-caption">Состояния</div>
                <div className="expr-grid">
                  {(
                    [
                      ["thinking", "Думает"],
                      ["listening", "Слушает"],
                      ["happy", "Радость"],
                      ["error", "Ошибка"],
                      ["sleep", "Сон"],
                    ] as [EyeMood, string][]
                  ).map(([m, label]) => (
                    <button key={m} className="expr-chip" onClick={() => showMood(m)}>
                      {label}
                    </button>
                  ))}
                </div>
              </Group>
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
                    const pKey = `tts:${st.id}`;
                    return (
                      <div key={st.id} className={`style${on ? " is-on" : ""}`}>
                        <button
                          role="radio"
                          aria-checked={on}
                          className="style-pick"
                          onClick={() => set((s) => ({ ...s, persona: { ...s.persona, style: st.id } }))}
                        >
                          <span className="style-label">{st.label}</span>
                          <span className="style-sample">«{sample}»</span>
                        </button>
                        <button
                          className="play"
                          aria-label={`Послушать: ${st.label}`}
                          onClick={() =>
                            preview.playing === pKey
                              ? preview.stop()
                              : void preview.play(st.id, draft.voice.ttsVoice, draft.voice.ttsRate, sample)
                          }
                        >
                          {preview.playing === pKey ? <span className="play-stop" /> : <span className="play-tri" />}
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
              <section className="tts-studio-card" aria-label="Голос">
                <div className="tts-studio-head">
                  <div className="tts-studio-title-wrap">
                    <h2 className="tts-studio-title">Голос Джарвиса</h2>
                    <p className="tts-studio-desc">
                      Работают сразу. Без интернета Джарвис говорит скачанным голосом ({platformCopy.localTts}).
                    </p>
                  </div>
                </div>

                <div className="tts-hd-grid" role="radiogroup" aria-label="Голоса Edge Neural">
                  {CURATED_EDGE_VOICES.map((cv) => {
                    const selected = draft.voice.ttsVoice === cv.id;
                    const pKey = `tts:hd-${cv.id}`;
                    const isPlayingThis = preview.playing === pKey;
                    return (
                      <div key={cv.id} className={`tts-hd-card${selected ? " is-selected" : ""}`}>
                        <button
                          type="button"
                          role="radio"
                          aria-checked={selected}
                          className="tts-hd-pick"
                          onClick={() => set((s) => ({ ...s, voice: { ...s.voice, ttsVoice: cv.id } }))}
                        >
                          <div className="tts-hd-top">
                            <span className="tts-hd-mark" />
                            <span className="tts-hd-name">{cv.shortName}</span>
                            <span className="tts-hd-badge">{cv.badge}</span>
                          </div>
                          <div className="tts-hd-desc">{cv.desc}</div>
                        </button>
                        <button
                          type="button"
                          className={`tts-hd-play${isPlayingThis ? " is-playing" : ""}`}
                          aria-label={isPlayingThis ? `Остановить ${cv.shortName}` : `Прослушать ${cv.shortName}`}
                          onClick={() =>
                            isPlayingThis
                              ? preview.stop()
                              : void preview.play(`hd-${cv.id}`, cv.id, draft.voice.ttsRate, samplePhrase)
                          }
                        >
                          {isPlayingThis ? (
                            <>
                              <LiveAudioBars active />
                              <span>Стоп</span>
                            </>
                          ) : (
                            <>
                              <WsIcon name="play" size={11} />
                              <span>Тест</span>
                            </>
                          )}
                        </button>
                      </div>
                    );
                  })}
                </div>

                <div className="tts-bench-box">
                  <div className="tts-bench-label-row">
                    <span className="tts-bench-label">Проверить</span>
                    <span className="tts-bench-active-voice mono">{voiceName}</span>
                  </div>
                  <div className="tts-bench-input-row">
                    <input
                      value={samplePhrase}
                      onChange={(e) => setSamplePhrase(e.target.value)}
                      placeholder="Фраза, которую я скажу…"
                      aria-label="Тестовая фраза для озвучки"
                    />
                    {ttsSpeaking || preview.playing ? (
                      <button
                        type="button"
                        className="btn tts-bench-stop-btn"
                        onClick={() => preview.stop()}
                      >
                        <LiveAudioBars active />
                        <span>Стоп</span>
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={!samplePhrase.trim()}
                        onClick={() =>
                          void preview.play(
                            "studio-sample",
                            draft.voice.ttsVoice || "edge:ru-RU-DmitryNeural",
                            draft.voice.ttsRate,
                            samplePhrase,
                          )
                        }
                      >
                        <WsIcon name="speaker" size={14} />
                        <span>Прослушать</span>
                      </button>
                    )}
                  </div>
                </div>
              </section>

              <Group title="Речь">
                <Row
                  label="Озвучивать ответы в чате"
                  hint="Читать вслух ответы, написанные в чате."
                >
                  <Switch
                    label="Озвучивать ответы в чате"
                    checked={draft.voice.autoSpeakReplies}
                    onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, autoSpeakReplies: v } }))}
                  />
                </Row>
                <Row
                  label="Скорость речи"
                  hint={`Диапазон 0,6× – 1,8× · сейчас ${draft.voice.ttsRate.toFixed(2).replace(".", ",")}×`}
                >
                  <div className="inline">
                    <input
                      className="slider"
                      type="range"
                      min={0.6}
                      max={1.8}
                      step={0.05}
                      value={draft.voice.ttsRate}
                      aria-label="Скорость речи"
                      onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, ttsRate: Number(e.target.value) } }))}
                    />
                    <span className="slider-readout mono">{draft.voice.ttsRate.toFixed(2).replace(".", ",")}×</span>
                  </div>
                </Row>
              </Group>

              <Group title="Разговор">
                <Row label="Голосовой режим" hint="Ctrl+Alt+K или кнопка микрофона на острове.">
                  <Switch
                    label="Голосовой режим"
                    checked={draft.voice.enabled}
                    onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, enabled: v } }))}
                  />
                </Row>
                {draft.voice.engine === "local" && (
                  <Row
                    label="Откликаться на имя"
                    hint="Скажите «Джарвис, …». Выполнив просьбу, он снова ждёт имени. «Пауза», «дальше», «назад» работают и без имени."
                  >
                    <Switch
                      label="Откликаться на имя"
                      checked={draft.voice.wakeWord}
                      onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, wakeWord: v } }))}
                    />
                  </Row>
                )}
                {draft.voice.engine === "local" && (
                  <Row label="Приглушать музыку" hint="Пока вы говорите с Джарвисом, остальные приложения звучат тише.">
                    <Switch
                      label="Приглушать музыку"
                      checked={draft.voice.duckMedia}
                      onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, duckMedia: v } }))}
                    />
                  </Row>
                )}
              </Group>

                <>
                  {sttModels.length > 0 && (
                    <Group title="Распознавание речи">
                      {sttModels.map((m) => (
                        <Row
                          key={m.id}
                          label={m.label}
                          hint="Нужна для голосового режима, работает без интернета."
                        >
                          <DownloadControl model={m} progress={progress} onDownload={() => void download(m.id)} />
                        </Row>
                      ))}
                    </Group>
                  )}

                  <Group title="Голоса без интернета">
                    <div className="voices" role="radiogroup" aria-label="Голос Джарвиса">
                      {nonEdgeVoices.map((v) => {
                        const on = draft.voice.ttsVoice === v.id;
                        const key = `voice:${v.id}`;
                        const pKey = `tts:${key}`;
                        return (
                          <div key={v.id || "auto"} className={`voice${on ? " is-on" : ""}`}>
                            <button
                              role="radio"
                              aria-checked={on}
                              className="voice-pick"
                              onClick={() => set((s) => ({ ...s, voice: { ...s.voice, ttsVoice: v.id } }))}
                            >
                              <span className="voice-mark" aria-hidden="true" />
                              <span className="voice-name">{v.name}</span>
                            </button>
                            <button
                              className="play"
                              aria-label={`Послушать: ${v.name}`}
                              onClick={() =>
                                preview.playing === pKey
                                  ? preview.stop()
                                  : void preview.play(key, v.id, draft.voice.ttsRate, samplePhrase)
                              }
                            >
                              {preview.playing === pKey ? <span className="play-stop" /> : <span className="play-tri" />}
                            </button>
                          </div>
                        );
                      })}
                    </div>
                    {preview.error && <p className="error-text">{preview.error}</p>}
                    {ttsModels.some((m) => !m.installed) && (
                      <div className="packs">
                        {ttsModels
                          .filter((m) => !m.installed)
                          .map((m) => (
                            <Row key={m.id} label={m.label} hint="Офлайн-голос для работы без интернета.">
                              <DownloadControl model={m} progress={progress} onDownload={() => void download(m.id)} />
                            </Row>
                          ))}
                      </div>
                    )}
                  </Group>

                  <details className="more">
                    <summary>Облачные голоса: Яндекс, ElevenLabs, Azure</summary>
                    <YandexVoice
                      has={view.hasYandexKey}
                      notify={notify}
                      onChanged={loadVoices}
                      onUse={(id) => void useCloudVoice(id)}
                    />
                    <ElevenVoice
                      has={view.hasElevenlabsKey}
                      notify={notify}
                      onChanged={loadVoices}
                      onUse={(id) => void useCloudVoice(id)}
                    />
                    <AzureVoice
                      has={view.hasAzureKey}
                      notify={notify}
                      onChanged={loadVoices}
                      onUse={(id) => void useCloudVoice(id)}
                    />
                  </details>
                </>

              <Group title="Устройства">
                <Row label="Микрофон">
                  <select
                    value={draft.voice.inputDevice}
                    onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, inputDevice: e.target.value } }))}
                  >
                    <option value="">По умолчанию{devices?.defaultInput ? ` (${devices.defaultInput})` : ""}</option>
                    {devices?.inputs.map((d) => (
                      <option key={d} value={d}>
                        {d}
                      </option>
                    ))}
                  </select>
                </Row>
                <Row label="Вывод звука">
                  <select
                    value={draft.voice.outputDevice}
                    onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, outputDevice: e.target.value } }))}
                  >
                    <option value="">По умолчанию{devices?.defaultOutput ? ` (${devices.defaultOutput})` : ""}</option>
                    {devices?.outputs.map((d) => (
                      <option key={d} value={d}>
                        {d}
                      </option>
                    ))}
                  </select>
                </Row>
              </Group>
            </>
          )}

          {section === "model" && (
            <Group>
              <Row label="Провайдер">
                <select
                  value={draft.provider.preset}
                  onChange={(e) => {
                    const p = view.presets.find((x) => x.id === e.target.value)!;
                    set((s) => ({
                      ...s,
                      provider: { ...s.provider, preset: p.id, baseUrl: "", model: p.defaultModel || s.provider.model },
                    }));
                  }}
                >
                  {view.presets.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </Row>
              {preset.needsKey && (
                <KeyField
                  kind="llm"
                  has={view.hasLlmKey}
                  baseUrl={draft.provider.baseUrl.trim() || preset.baseUrl}
                  onChanged={() => void load()}
                  notify={notify}
                />
              )}
              <details className="more">
                <summary>Адрес API и модель для изображений</summary>
              <Row label="Адрес API" hint={preset.baseUrl ? `Пусто — ${preset.baseUrl}` : "Укажите адрес OpenAI-совместимого API."}>
                <input
                  className="mono"
                  value={draft.provider.baseUrl}
                  placeholder={preset.baseUrl}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, baseUrl: e.target.value } }))}
                />
              </Row>
              <Row
                label="Модель"
                hint={
                  preset.id === "deepseek"
                    ? "V4.1 Flash: быстрые ответы, понимание экрана и поиск в интернете."
                    : preset.dashscope
                      ? "qwen3.8-flash: быстрее, qwen3.8-max: сильнее."
                      : undefined
                }
              >
                <input
                  className="mono"
                  value={draft.provider.model}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, model: e.target.value } }))}
                />
              </Row>
              <Row label="Модель для изображений" hint="Пусто — та же модель.">
                <input
                  className="mono"
                  value={draft.provider.visionModel}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, visionModel: e.target.value } }))}
                />
              </Row>
              </details>
              <Row label="Проверить подключение" hint="Короткий запрос к модели с этим ключом.">
                <button className="btn" disabled={busy} onClick={() => void test()}>
                  {busy ? "Проверяю…" : "Проверить"}
                </button>
              </Row>
            </Group>
          )}

          {section === "behaviour" && (
            <>
              <section className={`autopilot-hero-card${draft.agent.fullControl ? " is-on" : ""}`}>
                <div className="autopilot-hero-main">
                  <div className="autopilot-hero-badge">
                    <WsIcon name="bolt" size={12} />
                    <span>Автопилот</span>
                  </div>
                  <h2>Полное управление</h2>
                  <p>
                    Окна, программы, файлы и таймеры — без лишних вопросов. Удаление, покупки и отправку сообщений Джарвис всё равно
                    сначала покажет {vy ? "вам" : "тебе"}.
                  </p>
                </div>
                <div className="autopilot-hero-switch">
                  <Switch
                    label="Полное управление (Автопилот)"
                    checked={draft.agent.fullControl}
                    onChange={(v) => void toggleFullControlLive(v)}
                  />
                </div>
              </section>

              <Group title="Разрешения">
                {!draft.agent.fullControl && (
                  <Row
                    label="Спрашивать перед изменениями"
                    hint="Чтение файлов, команды, меняющие проект, закрытие окон. Отправка сообщений, удаление и покупки подтверждаются всегда."
                  >
                    <Switch
                      label="Спрашивать перед изменениями"
                      checked={draft.agent.confirmModerate}
                      onChange={(v) => set((s) => ({ ...s, agent: { ...s.agent, confirmModerate: v } }))}
                    />
                  </Row>
                )}
                <Row label="Папки проектов" hint="По одной на строке: здесь Джарвис ищет код, отсюда берёт коммиты для «Стрика».">
                  <textarea
                    className="mono"
                    rows={3}
                    value={draft.agent.projectRoots.join("\n")}
                    spellCheck={false}
                    onChange={(e) =>
                      set((s) => ({
                        ...s,
                        agent: {
                          ...s.agent,
                          projectRoots: e.target.value
                            .split("\n")
                            .map((l) => l.trim())
                            .filter(Boolean),
                        },
                      }))
                    }
                  />
                </Row>
              </Group>

              <RoutinesSettingsManager notify={notify} />
            </>
          )}

          {section === "mcp" && <McpSection notify={notify} vy={vy} />}

          {section === "general" && (
            <>
              <Group title="Запуск">
                <Row label={platformCopy.startup}>
                  <Switch
                    label={platformCopy.startup}
                    checked={draft.launchAtStartup}
                    onChange={(v) => set((s) => ({ ...s, launchAtStartup: v }))}
                  />
                </Row>
                <Row label="Меньше движения" hint="Мгновенные переходы вместо пружинной анимации.">
                  <Switch label="Меньше движения" checked={draft.reduceMotion} onChange={(v) => set((s) => ({ ...s, reduceMotion: v }))} />
                </Row>
              </Group>

              <Group title="Обновления">
                <Row
                  label={updates.status?.available ? `Доступна версия ${updates.status.available.version}` : "Установлена последняя версия"}
                  hint={
                    updates.error ??
                    (updates.progress
                      ? updates.progress.total
                        ? `Скачиваю — ${Math.round((updates.progress.downloaded / updates.progress.total) * 100)}%`
                        : "Скачиваю…"
                      : updates.status?.available
                        ? "Скачается, проверится подписью и установится, затем Джарвис перезапустится."
                        : updates.status
                          ? `Сейчас ${updates.status.current}${updates.status.checkedAt ? `, проверено в ${new Date(updates.status.checkedAt).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" })}` : ""}`
                          : undefined)
                  }
                >
                  {updates.status?.available ? (
                    <button className="btn btn-primary" disabled={updates.progress !== null} onClick={() => void updates.install()}>
                      {updates.progress ? "Устанавливаю…" : "Обновить и перезапустить"}
                    </button>
                  ) : (
                    <button className="btn" disabled={updates.checking} onClick={() => void updates.check()}>
                      {updates.checking ? "Проверяю…" : "Проверить"}
                    </button>
                  )}
                </Row>
                <Row label="Проверять автоматически" hint="Раз в шесть часов. Устанавливается только по вашей команде.">
                  <Switch
                    label="Проверять обновления автоматически"
                    checked={draft.updates.autoCheck}
                    onChange={(v) => set((s) => ({ ...s, updates: { ...s.updates, autoCheck: v } }))}
                  />
                </Row>
              </Group>

              <Group title="Сочетания клавиш">
                <Row label="Открыть или свернуть чат" hint="Например, Ctrl+Alt+J. Сочетания с Win заняты системой.">
                  <div className="inline">
                    <input
                      className="mono narrow-wide"
                      value={draft.shortcuts.workspace}
                      spellCheck={false}
                      onChange={(e) => set((s) => ({ ...s, shortcuts: { ...s.shortcuts, workspace: e.target.value } }))}
                    />
                    <Keycaps value={draft.shortcuts.workspace} />
                  </div>
                </Row>
                <Row label="Голосовой режим">
                  <div className="inline">
                    <input
                      className="mono narrow-wide"
                      value={draft.shortcuts.voice}
                      spellCheck={false}
                      onChange={(e) => set((s) => ({ ...s, shortcuts: { ...s.shortcuts, voice: e.target.value } }))}
                    />
                    <Keycaps value={draft.shortcuts.voice} />
                  </div>
                </Row>
                <Row label="История буфера обмена" hint="Открывает историю с поиском: Enter вставляет, Esc закрывает.">
                  <div className="inline">
                    <input
                      className="mono narrow-wide"
                      value={draft.shortcuts.clipboard}
                      spellCheck={false}
                      onChange={(e) => set((s) => ({ ...s, shortcuts: { ...s.shortcuts, clipboard: e.target.value } }))}
                    />
                    <Keycaps value={draft.shortcuts.clipboard} />
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
              <Group title="О программе">
                <Row label="Журнал" wide>
                  <span className="value mono path">{view.logPath}</span>
                </Row>
                <Row label="Данные" wide>
                  <span className="value mono path">{view.dataPath}</span>
                </Row>
              </Group>
            </>
          )}

          {section === "privacy" && (
            <>
              <Group>
                <Row label="Хранить историю разговоров" hint="Локально, в базе на этом компьютере. Снимки экрана и звук не сохраняются никогда.">
                  <Switch
                    label="Хранить историю разговоров"
                    checked={draft.privacy.storeHistory}
                    onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, storeHistory: v } }))}
                  />
                </Row>
                <Row label="Разрешить снимки экрана" hint="Только по ходу задачи, которой это нужно. Снимок уходит выбранной модели.">
                  <Switch
                    label="Разрешить снимки экрана"
                    checked={draft.privacy.allowScreen}
                    onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, allowScreen: v } }))}
                  />
                </Row>
                <Row label="Читать буфер обмена без вопроса">
                  <Switch
                    label="Читать буфер обмена без вопроса"
                    checked={draft.privacy.allowClipboardRead}
                    onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, allowClipboardRead: v } }))}
                  />
                </Row>
              </Group>
              <Group title="История буфера обмена">
                <Row
                  label="Запоминать, что копируется"
                  hint="Текст, картинки и списки файлов — только на этом компьютере, в той же базе. Если выключить, буфер вообще не читается."
                >
                  <Switch
                    label="Запоминать, что копируется"
                    checked={draft.clipboard.enabled}
                    onChange={(v) => set((s) => ({ ...s, clipboard: { ...s.clipboard, enabled: v } }))}
                  />
                </Row>
                <Row
                  label="Не запоминать похожее на пароль"
                  hint="Ключи API, токены, закрытые ключи и номера банковских карт пропускаются. Менеджеры паролей и так помечают свои копии как приватные — их я не читаю."
                >
                  <Switch
                    label="Не запоминать похожее на пароль"
                    checked={draft.clipboard.skipSecrets}
                    onChange={(v) => set((s) => ({ ...s, clipboard: { ...s.clipboard, skipSecrets: v } }))}
                  />
                </Row>
                <Row label="Программы, которые не слушать" hint="Часть имени процесса, по одной на строке: keepass подойдёт к KeePassXC.exe.">
                  <textarea
                    className="mono"
                    rows={4}
                    value={draft.clipboard.ignoredApps.join("\n")}
                    spellCheck={false}
                    onChange={(e) =>
                      set((s) => ({
                        ...s,
                        clipboard: {
                          ...s.clipboard,
                          ignoredApps: e.target.value
                            .split("\n")
                            .map((l) => l.trim())
                            .filter(Boolean),
                        },
                      }))
                    }
                  />
                </Row>
                <Row label="Сколько записей хранить" hint="Закреплённые не считаются и не удаляются сами.">
                  <input
                    className="narrow"
                    type="number"
                    min={10}
                    max={1000}
                    value={draft.clipboard.maxItems}
                    onChange={(e) =>
                      set((s) => ({
                        ...s,
                        clipboard: { ...s.clipboard, maxItems: Math.min(1000, Math.max(10, Number(e.target.value) || 100)) },
                      }))
                    }
                  />
                </Row>
                <Row label="Сколько дней хранить" hint="0 — пока не вытеснят новые.">
                  <input
                    className="narrow"
                    type="number"
                    min={0}
                    max={365}
                    value={draft.clipboard.keepDays}
                    onChange={(e) =>
                      set((s) => ({
                        ...s,
                        clipboard: { ...s.clipboard, keepDays: Math.min(365, Math.max(0, Number(e.target.value) || 0)) },
                      }))
                    }
                  />
                </Row>
                <div className="actions">
                  <button
                    className="btn btn-danger"
                    onClick={() =>
                      void invoke<number>("clips_clear", { keepPinned: true }).then((n) =>
                        notify("ok", n ? `Из истории буфера удалено: ${n}` : "История буфера уже пуста"),
                      )
                    }
                  >
                    Очистить историю буфера
                  </button>
                </div>
              </Group>
            </>
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
                        <button
                          className="btn btn-danger"
                          onClick={() =>
                            void invoke("memory_delete", { id: m.id }).then(() => invoke<Memory[]>("memories_list").then(setMemories))
                          }
                        >
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
                          {new Date(t.startedAt).toLocaleString("ru-RU", {
                            day: "2-digit",
                            month: "2-digit",
                            hour: "2-digit",
                            minute: "2-digit",
                          })}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
                <div className="actions">
                  <button
                    className="btn btn-danger"
                    onClick={() => void invoke("history_clear").then(() => (setTasks([]), notify("ok", "История очищена")))}
                  >
                    Очистить историю
                  </button>
                </div>
              </Group>
            </>
          )}

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
                  <button className="btn" onClick={() => setDraft(structuredClone(view.settings))}>
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
