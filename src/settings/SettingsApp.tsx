// Settings window: model & key, voice, behaviour, shortcuts, privacy,
// memory/history and diagnostics. Secrets go straight to Windows Credential
// Manager through `secret_set`; the UI only ever learns whether a key exists.

import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/ipc";
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
    realtimeUrl: string;
    model: string;
    voice: string;
    reuseLlmKey: boolean;
    inputDevice: string;
    outputDevice: string;
  };
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
  metrics: { lastTimeToFirstAudioMs: number | null; lastBargeInMs: number | null; connectMs: number | null };
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

type Section = "model" | "voice" | "behaviour" | "shortcuts" | "privacy" | "memory" | "diagnostics";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "model", label: "Модель" },
  { id: "voice", label: "Голос" },
  { id: "behaviour", label: "Поведение" },
  { id: "shortcuts", label: "Сочетания клавиш" },
  { id: "privacy", label: "Приватность" },
  { id: "memory", label: "Память и история" },
  { id: "diagnostics", label: "Диагностика" },
];

function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}

function Toggle({ checked, onChange, label, hint }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: string }) {
  return (
    <div className="toggle-row">
      <div>
        <div className="field-label">{label}</div>
        {hint && <div className="field-hint">{hint}</div>}
      </div>
      <button role="switch" aria-checked={checked} aria-label={label} className={`switch${checked ? " is-on" : ""}`} onClick={() => onChange(!checked)}>
        <span />
      </button>
    </div>
  );
}

function KeyField({ kind, has, onChanged }: { kind: "llm" | "voice"; has: boolean; onChanged: () => void }) {
  const [value, setValue] = useState("");
  const [editing, setEditing] = useState(!has);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => setEditing(!has), [has]);

  const save = async () => {
    try {
      await invoke("secret_set", { kind, value });
      setValue("");
      setMsg("Ключ сохранён в диспетчере учётных данных Windows");
      onChanged();
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  const clear = async () => {
    await invoke("secret_clear", { kind });
    setMsg("Ключ удалён");
    onChanged();
  };

  return (
    <Field label="API-ключ" hint={msg ?? "Хранится в диспетчере учётных данных Windows, не в файлах приложения."}>
      {editing ? (
        <div className="row">
          <input
            type="password"
            value={value}
            autoComplete="off"
            spellCheck={false}
            placeholder="sk-…"
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && value.trim() && void save()}
          />
          <button className="btn btn-solid" disabled={!value.trim()} onClick={() => void save()}>
            Сохранить ключ
          </button>
          {has && (
            <button className="btn btn-quiet" onClick={() => setEditing(false)}>
              Отмена
            </button>
          )}
        </div>
      ) : (
        <div className="row">
          <span className="key-set">Ключ сохранён</span>
          <button className="btn btn-quiet" onClick={() => setEditing(true)}>
            Заменить
          </button>
          <button className="btn btn-quiet danger" onClick={() => void clear()}>
            Удалить
          </button>
        </div>
      )}
    </Field>
  );
}

export default function SettingsApp() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [section, setSection] = useState<Section>("model");
  const [status, setStatus] = useState<{ tone: "ok" | "error" | "info"; text: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const [memories, setMemories] = useState<Memory[]>([]);
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [diag, setDiag] = useState<Diagnostics | null>(null);
  const [shortcutProblems, setShortcutProblems] = useState<string[]>([]);
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  const [voiceStatus, setVoiceStatus] = useState<VoiceStatus | null>(null);

  const load = async () => {
    const v = await invoke<SettingsView>("settings_get");
    setView(v);
    setDraft(v.settings);
  };
  useEffect(() => {
    void load();
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

  const dirty = useMemo(() => JSON.stringify(view?.settings) !== JSON.stringify(draft), [view, draft]);
  if (!view || !draft) return <div className="settings-loading" />;

  const preset = view.presets.find((p) => p.id === draft.provider.preset) ?? view.presets[0]!;
  const set = (patch: (s: Settings) => Settings) => setDraft((d) => (d ? patch(structuredClone(d)) : d));

  const save = async () => {
    try {
      await invoke("settings_save", { settings: draft });
      setStatus({ tone: "ok", text: "Изменения сохранены" });
      await load();
      setShortcutProblems(await invoke<string[]>("shortcut_problems"));
    } catch (e) {
      setStatus({ tone: "error", text: errorMessage(e) });
    }
  };

  const test = async () => {
    setTesting(true);
    setStatus({ tone: "info", text: "Проверяю подключение…" });
    try {
      if (dirty) await invoke("settings_save", { settings: draft });
      const r = await invoke<{ latencyMs: number; model: string; reply: string }>("provider_test");
      setStatus({ tone: "ok", text: `Модель ${r.model} отвечает: «${r.reply}» за ${r.latencyMs} мс` });
      await load();
    } catch (e) {
      setStatus({ tone: "error", text: errorMessage(e) });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="settings">
      <nav className="side">
        <div className="side-brand">
          <span className="brand-eyes" aria-hidden="true">
            <i />
            <i />
          </span>
          JARVIS
        </div>
        {SECTIONS.map((s) => (
          <button key={s.id} className={`side-item${section === s.id ? " is-active" : ""}`} onClick={() => setSection(s.id)}>
            {s.label}
          </button>
        ))}
      </nav>

      <main className="content scroll">
        {section === "model" && (
          <section>
            <h1>Модель</h1>
            <p className="lead">Джарвис понимает запросы и выбирает действия с помощью языковой модели с вызовом инструментов.</p>
            <Field label="Провайдер">
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
            </Field>
            <Field label="Адрес API" hint={`По умолчанию: ${preset.baseUrl || "укажите адрес"}. Для домена рабочего пространства Model Studio вставьте его сюда.`}>
              <input
                value={draft.provider.baseUrl}
                placeholder={preset.baseUrl}
                spellCheck={false}
                onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, baseUrl: e.target.value } }))}
              />
            </Field>
            <div className="grid2">
              <Field label="Модель" hint={preset.dashscope ? "qwen3.8-flash — быстрая, qwen3.8-max — сильнее" : undefined}>
                <input
                  value={draft.provider.model}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, model: e.target.value } }))}
                />
              </Field>
              <Field label="Модель для изображений" hint="Пусто — та же модель">
                <input
                  value={draft.provider.visionModel}
                  spellCheck={false}
                  onChange={(e) => set((s) => ({ ...s, provider: { ...s.provider, visionModel: e.target.value } }))}
                />
              </Field>
            </div>
            {preset.needsKey && <KeyField kind="llm" has={view.hasLlmKey} onChanged={() => void load()} />}
            <div className="row">
              <button className="btn btn-quiet" disabled={testing} onClick={() => void test()}>
                Проверить подключение
              </button>
            </div>
          </section>
        )}

        {section === "voice" && (
          <section>
            <h1>Голос</h1>
            <p className="lead">Разговор в реальном времени через Qwen Omni Realtime: речь в обе стороны, перебивание голосом.</p>
            <Toggle checked={draft.voice.enabled} onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, enabled: v } }))} label="Голосовой режим" />
            <Field label="Адрес realtime" hint="WebSocket-адрес Model Studio без параметра ?model=">
              <input value={draft.voice.realtimeUrl} spellCheck={false} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, realtimeUrl: e.target.value } }))} />
            </Field>
            <div className="grid2">
              <Field label="Модель">
                <input value={draft.voice.model} spellCheck={false} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, model: e.target.value } }))} />
              </Field>
              <Field label="Голос" hint="Для русского: Katerina, Marina, Alek, Tina">
                <input value={draft.voice.voice} spellCheck={false} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, voice: e.target.value } }))} />
              </Field>
            </div>
            <Toggle
              checked={draft.voice.reuseLlmKey}
              onChange={(v) => set((s) => ({ ...s, voice: { ...s.voice, reuseLlmKey: v } }))}
              label="Использовать ключ модели"
              hint="Один аккаунт Model Studio для текста и голоса"
            />
            {!draft.voice.reuseLlmKey && <KeyField kind="voice" has={view.hasVoiceKey} onChanged={() => void load()} />}
            <div className="grid2">
              <Field label="Микрофон">
                <select value={draft.voice.inputDevice} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, inputDevice: e.target.value } }))}>
                  <option value="">По умолчанию{devices?.defaultInput ? ` — ` : ""}</option>
                  {devices?.inputs.map((d) => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Вывод звука">
                <select value={draft.voice.outputDevice} onChange={(e) => set((s) => ({ ...s, voice: { ...s.voice, outputDevice: e.target.value } }))}>
                  <option value="">По умолчанию{devices?.defaultOutput ? ` — ` : ""}</option>
                  {devices?.outputs.map((d) => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
              </Field>
            </div>
            <p className="field-hint">С колонками без наушников Джарвис приглушает микрофон, пока говорит сам, — перебивать можно, говоря громче.</p>
          </section>
        )}

        {section === "behaviour" && (
          <section>
            <h1>Поведение</h1>
            <Toggle
              checked={draft.agent.confirmModerate}
              onChange={(v) => set((s) => ({ ...s, agent: { ...s.agent, confirmModerate: v } }))}
              label="Спрашивать перед изменениями"
              hint="Чтение файлов, команды, меняющие проект, закрытие окон. Опасные действия подтверждаются всегда."
            />
            <Field label="Папки проектов" hint="По одной на строке. Помогают понять «мой проект».">
              <textarea
                rows={4}
                value={draft.agent.projectRoots.join("\n")}
                spellCheck={false}
                onChange={(e) => set((s) => ({ ...s, agent: { ...s.agent, projectRoots: e.target.value.split("\n").map((l) => l.trim()).filter(Boolean) } }))}
              />
            </Field>
            <Field label="Предел шагов на задачу">
              <input
                type="number"
                min={4}
                max={60}
                value={draft.agent.maxSteps}
                onChange={(e) => set((s) => ({ ...s, agent: { ...s.agent, maxSteps: Number(e.target.value) || 24 } }))}
              />
            </Field>
            <Toggle checked={draft.launchAtStartup} onChange={(v) => set((s) => ({ ...s, launchAtStartup: v }))} label="Запускать вместе с Windows" />
            <Toggle checked={draft.reduceMotion} onChange={(v) => set((s) => ({ ...s, reduceMotion: v }))} label="Меньше движения" hint="Мгновенные переходы вместо пружинной анимации" />
          </section>
        )}

        {section === "shortcuts" && (
          <section>
            <h1>Сочетания клавиш</h1>
            <p className="lead">Формат: Ctrl+Alt+J, Ctrl+Shift+Space. Сочетания с Win зарезервированы системой.</p>
            <Field label="Открыть или свернуть чат">
              <input value={draft.shortcuts.workspace} spellCheck={false} onChange={(e) => set((s) => ({ ...s, shortcuts: { ...s.shortcuts, workspace: e.target.value } }))} />
            </Field>
            <Field label="Голосовой режим">
              <input value={draft.shortcuts.voice} spellCheck={false} onChange={(e) => set((s) => ({ ...s, shortcuts: { ...s.shortcuts, voice: e.target.value } }))} />
            </Field>
            {shortcutProblems.length > 0 && (
              <div className="callout error">
                {shortcutProblems.map((p) => (
                  <div key={p}>{p}</div>
                ))}
              </div>
            )}
            <p className="field-hint">Esc закрывает чат. Остановить задачу — кнопка в чате или пункт в меню трея.</p>
          </section>
        )}

        {section === "privacy" && (
          <section>
            <h1>Приватность</h1>
            <Toggle
              checked={draft.privacy.storeHistory}
              onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, storeHistory: v } }))}
              label="Хранить историю разговоров"
              hint="Локально, в базе SQLite. Скриншоты и звук не сохраняются никогда."
            />
            <Toggle
              checked={draft.privacy.allowScreen}
              onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, allowScreen: v } }))}
              label="Разрешить снимки экрана"
              hint="Только по ходу задачи, которая этого требует. Снимок уходит выбранной модели."
            />
            <Toggle
              checked={draft.privacy.allowClipboardRead}
              onChange={(v) => set((s) => ({ ...s, privacy: { ...s.privacy, allowClipboardRead: v } }))}
              label="Читать буфер обмена без вопроса"
            />
          </section>
        )}

        {section === "memory" && (
          <section>
            <h1>Память и история</h1>
            <h2>Что Джарвис запомнил</h2>
            {memories.length === 0 ? (
              <p className="field-hint">Пока ничего. Скажите «запомни, что…», и запись появится здесь.</p>
            ) : (
              <ul className="list">
                {memories.map((m) => (
                  <li key={m.id}>
                    <span>{m.text}</span>
                    <button
                      className="btn btn-quiet danger"
                      onClick={() => void invoke("memory_delete", { id: m.id }).then(() => invoke<Memory[]>("memories_list").then(setMemories))}
                    >
                      Забыть
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <h2>Последние задачи</h2>
            {tasks.length === 0 ? (
              <p className="field-hint">Задач ещё не было.</p>
            ) : (
              <ul className="list tasks">
                {tasks.map((t) => (
                  <li key={t.id}>
                    <span className={`dot dot-${t.status}`} aria-label={t.status} />
                    <span className="task-text">{t.request}</span>
                    <span className="task-meta">
                      {new Date(t.startedAt).toLocaleString("ru-RU", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" })} · {t.steps} шаг.
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <div className="row">
              <button
                className="btn btn-quiet danger"
                onClick={() =>
                  void invoke("history_clear").then(() => {
                    setTasks([]);
                    setStatus({ tone: "ok", text: "История очищена" });
                  })
                }
              >
                Очистить историю
              </button>
            </div>
          </section>
        )}

        {section === "diagnostics" && (
          <section>
            <h1>Диагностика</h1>
            {diag ? (
              <dl className="diag">
                <dt>Время работы</dt>
                <dd>{Math.round(diag.uptimeSecs / 60)} мин</dd>
                <dt>Память процесса (без WebView2)</dt>
                <dd>{diag.workingSetMb.toFixed(1)} МБ</dd>
                <dt>Частота трекера курсора</dt>
                <dd>{diag.trackerHz.toFixed(1)} Гц</dd>
                <dt>Мониторы</dt>
                <dd>
                  {diag.monitors
                    .map((m) => `${m.name.replace("\\\\.\\", "")} ${m.bounds.width}×${m.bounds.height} @${Math.round(m.scale * 100)}%${m.primary ? " (основной)" : ""}`)
                    .join(", ")}
                </dd>
                <dt>Голос: время до первого звука</dt>
                <dd>{voiceStatus?.metrics.lastTimeToFirstAudioMs != null ? `${voiceStatus.metrics.lastTimeToFirstAudioMs} мс` : "ещё не измерено"}</dd>
                <dt>Голос: подключение</dt>
                <dd>{voiceStatus?.metrics.connectMs != null ? `${voiceStatus.metrics.connectMs} мс` : "ещё не измерено"}</dd>
                <dt>Журнал</dt>
                <dd className="mono">{view.logPath}</dd>
                <dt>Данные</dt>
                <dd className="mono">{view.dataPath}</dd>
              </dl>
            ) : (
              <p className="field-hint">Измеряю…</p>
            )}
          </section>
        )}
      </main>

      <footer className="bar">
        <span className={`status ${status?.tone ?? ""}`} role="status">
          {status?.text}
        </span>
        <button className="btn btn-quiet" disabled={!dirty} onClick={() => setDraft(view.settings)}>
          Отменить
        </button>
        <button className="btn btn-solid" disabled={!dirty} onClick={() => void save()}>
          Сохранить изменения
        </button>
      </footer>
    </div>
  );
}
