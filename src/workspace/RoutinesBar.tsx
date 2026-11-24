// Executive Routines & Quick Action Bar for WorkspaceView.
// Loads persistent playbooks from `hub_routines_list` (with fallback only when
// outside Tauri), executes via `hub_routine_run` or `agent.send`, provides the
// "Озвучить выделенное" quick action, and supports inline routine creation
// via `hub_routine_save`.

import { useEffect, useState } from "react";
import { useAgent } from "../store/agent";
import { usePrefs } from "../store/prefs";
import { formatNextDue, reminderCancel, useScheduledJobs, type Reminder } from "../store/reminders";
import { useWorkspaceRoutines, workspaceIpc, type RoutineItem } from "../lib/workspace-ipc";
import { routineIconName, WsIcon } from "./WorkspaceIcons";

const ICON_OPTIONS = [
  { id: "target", label: "Фокус" },
  { id: "sun", label: "Брифинг" },
  { id: "window", label: "Окна" },
  { id: "cpu", label: "Система" },
  { id: "bolt", label: "Авто" },
] as const;

const SPEAK_SELECTION_PROMPT =
  "Захвати выделенный текст в активном окне через selection_grab и озвучь его вслух через speak_aloud.";

function ScheduledList({ jobs, compact = false }: { jobs: Reminder[]; compact?: boolean }) {
  if (jobs.length === 0) return null;
  return (
    <div className={`scheduled-box${compact ? " is-compact" : ""}`} aria-label="Задачи по расписанию">
      <div className="scheduled-head">
        <WsIcon name="repeat" size={11} className="scheduled-head-icon" />
        <span>По расписанию</span>
        <span className="scheduled-count">{jobs.length}</span>
      </div>
      <div className="scheduled-list">
        {jobs.map((job) => (
          <div key={job.id} className="scheduled-row">
            <div className="scheduled-main">
              <span className="scheduled-text" title={job.text}>{job.text}</span>
              <span className="scheduled-meta">
                {job.repeatLabel && <span className="scheduled-rule">{job.repeatLabel}</span>}
                <span className="scheduled-next">след. {formatNextDue(job.dueAt)}</span>
              </span>
            </div>
            <button
              type="button"
              className="scheduled-cancel"
              aria-label={`Отменить задачу «${job.text}»`}
              title="Остановить серию"
              onClick={() => void reminderCancel(job.id)}
            >
              <WsIcon name="close" size={11} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

export function RoutinesBar({ variant = "empty" }: { variant?: "empty" | "strip" }) {
  const routines = useWorkspaceRoutines((s) => s.routines);
  const loaded = useWorkspaceRoutines((s) => s.loaded);
  const loadRoutines = useWorkspaceRoutines((s) => s.load);
  const saveRoutine = useWorkspaceRoutines((s) => s.save);
  const activeTurn = useAgent((s) => s.activeTurn);
  const send = useAgent((s) => s.send);
  const fullControl = usePrefs((s) => s.fullControl);
  const scheduled = useScheduledJobs();

  const [creating, setCreating] = useState(false);
  const [showScheduled, setShowScheduled] = useState(false);
  const [title, setTitle] = useState("");
  const [subtitle, setSubtitle] = useState("");
  const [icon, setIcon] = useState<string>("bolt");
  const [prompt, setPrompt] = useState("");
  const [autonomy, setAutonomy] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!loaded) void loadRoutines();
  }, [loaded, loadRoutines]);

  const executeRoutine = async (r: RoutineItem) => {
    if (activeTurn) return;
    const res = await workspaceIpc.routines.run(r.id);
    if (!res) {
      await send(r.prompt, []);
    }
  };

  const triggerSpeakSelection = () => {
    if (activeTurn) return;
    void send(SPEAK_SELECTION_PROMPT, []);
  };

  const handleSave = async () => {
    if (!title.trim() || !prompt.trim() || saving) return;
    setSaving(true);
    try {
      await saveRoutine({
        title: title.trim(),
        subtitle: subtitle.trim() || "Исполнительный сценарий",
        icon,
        prompt: prompt.trim(),
        autonomy,
      });
      setTitle("");
      setSubtitle("");
      setPrompt("");
      setCreating(false);
    } finally {
      setSaving(false);
    }
  };

  if (variant === "strip") {
    return (
      <div className="routines-strip-wrap">
        <div className="routines-strip" role="toolbar" aria-label="Быстрые сценарии и действия">
          {routines.map((r) => (
            <button
              key={r.id}
              type="button"
              className="routine-chip"
              disabled={!!activeTurn}
              title={`${r.title}: ${r.subtitle}`}
              onClick={() => void executeRoutine(r)}
            >
              <WsIcon name={routineIconName(r.icon)} size={12} className="routine-chip-icon" />
              <span>{r.title}</span>
            </button>
          ))}
          <button
            type="button"
            className="routine-chip is-accent"
            disabled={!!activeTurn}
            title="Скопировать выделенный текст в активном окне и озвучить голосом Джарвиса"
            onClick={triggerSpeakSelection}
          >
            <WsIcon name="selection" size={12} />
            <span>Озвучить выделенное</span>
          </button>
          {scheduled.length > 0 && (
            <button
              type="button"
              className={`routine-chip${showScheduled ? " is-open" : ""}`}
              title="Регулярные задачи по расписанию"
              onClick={() => setShowScheduled((v) => !v)}
            >
              <WsIcon name="repeat" size={11} className="routine-chip-icon" />
              <span>По расписанию · {scheduled.length}</span>
            </button>
          )}
          <button
            type="button"
            className={`routine-chip is-add${creating ? " is-open" : ""}`}
            title="Создать свой сценарий"
            onClick={() => setCreating((v) => !v)}
          >
            <WsIcon name={creating ? "close" : "plus"} size={11} />
            <span>{creating ? "Закрыть" : "Сценарий"}</span>
          </button>
        </div>

        {showScheduled && <ScheduledList jobs={scheduled} compact />}

        {creating && (
          <div className="routine-inline-form">
            <div className="routine-form-row">
              <input
                value={title}
                placeholder="Название сценария (напр. Режим кода)"
                onChange={(e) => setTitle(e.target.value)}
              />
              <input
                value={subtitle}
                placeholder="Краткое описание"
                onChange={(e) => setSubtitle(e.target.value)}
              />
            </div>
            <textarea
              rows={2}
              value={prompt}
              placeholder="Поручение Джарвису: какие окна разложить, что проверить или запустить…"
              onChange={(e) => setPrompt(e.target.value)}
            />
            <div className="routine-form-foot">
              <div className="routine-icon-picks">
                {ICON_OPTIONS.map((o) => (
                  <button
                    key={o.id}
                    type="button"
                    className={`routine-icon-pick${icon === o.id ? " is-on" : ""}`}
                    onClick={() => setIcon(o.id)}
                    title={o.label}
                  >
                    <WsIcon name={routineIconName(o.id)} size={12} />
                  </button>
                ))}
              </div>
              <label className="routine-auto-toggle">
                <input type="checkbox" checked={autonomy} onChange={(e) => setAutonomy(e.target.checked)} />
                <span>Автопилот</span>
              </label>
              <button
                type="button"
                className="btn btn-solid routine-save-btn"
                disabled={!title.trim() || !prompt.trim() || saving}
                onClick={() => void handleSave()}
              >
                {saving ? "Сохраняю…" : "Сохранить"}
              </button>
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="ws-empty-deck">
      <div className="ws-empty-head">
        <div className="ws-empty-badge">
          <span className={`ws-mode-dot${fullControl ? " is-autopilot" : ""}`} />
          <span>{fullControl ? "Автопилот активен · Полное управление" : "Готов к поручениям"}</span>
        </div>
        <h2 className="ws-empty-title">Чем займёмся?</h2>
        <p className="ws-empty-sub">
          Запустите готовый исполнительный сценарий в один клик или опишите задачу своими словами.
        </p>
      </div>

      <div className="routines-grid">
        {routines.slice(0, 4).map((r) => (
          <button
            key={r.id}
            type="button"
            className="routine-card"
            disabled={!!activeTurn}
            onClick={() => void executeRoutine(r)}
          >
            <div className="routine-card-top">
              <span className="routine-card-icon">
                <WsIcon name={routineIconName(r.icon)} size={15} />
              </span>
              {r.autonomy && (
                <span className="routine-card-tag" title="Выполняется автономно">
                  <WsIcon name="bolt" size={10} />
                  Авто
                </span>
              )}
            </div>
            <div className="routine-card-title">{r.title}</div>
            <div className="routine-card-sub">{r.subtitle}</div>
          </button>
        ))}
      </div>

      <div className="routines-quick-row">
        <button
          type="button"
          className="routine-quick-pill is-highlight"
          disabled={!!activeTurn}
          onClick={triggerSpeakSelection}
        >
          <WsIcon name="speaker" size={13} />
          <span>Озвучить выделенное</span>
        </button>

        {routines.slice(4).map((r) => (
          <button
            key={r.id}
            type="button"
            className="routine-quick-pill"
            disabled={!!activeTurn}
            onClick={() => void executeRoutine(r)}
          >
            <WsIcon name={routineIconName(r.icon)} size={12} />
            <span>{r.title}</span>
          </button>
        ))}

        <button
          type="button"
          className={`routine-quick-pill${creating ? " is-active" : ""}`}
          onClick={() => setCreating((v) => !v)}
        >
          <WsIcon name={creating ? "close" : "plus"} size={12} />
          <span>{creating ? "Отмена" : "Новый сценарий"}</span>
        </button>
      </div>

      <ScheduledList jobs={scheduled} />

      {creating && (
        <div className="routine-inline-form">
          <div className="routine-form-row">
            <input
              value={title}
              placeholder="Название (напр. Чистый рабочий стол)"
              onChange={(e) => setTitle(e.target.value)}
            />
            <input
              value={subtitle}
              placeholder="Подзаголовок"
              onChange={(e) => setSubtitle(e.target.value)}
            />
          </div>
          <textarea
            rows={2}
            value={prompt}
            placeholder="Инструкция для Джарвиса…"
            onChange={(e) => setPrompt(e.target.value)}
          />
          <div className="routine-form-foot">
            <div className="routine-icon-picks">
              {ICON_OPTIONS.map((o) => (
                <button
                  key={o.id}
                  type="button"
                  className={`routine-icon-pick${icon === o.id ? " is-on" : ""}`}
                  onClick={() => setIcon(o.id)}
                  title={o.label}
                >
                  <WsIcon name={routineIconName(o.id)} size={12} />
                </button>
              ))}
            </div>
            <label className="routine-auto-toggle">
              <input type="checkbox" checked={autonomy} onChange={(e) => setAutonomy(e.target.checked)} />
              <span>Автопилот</span>
            </label>
            <button
              type="button"
              className="btn btn-solid routine-save-btn"
              disabled={!title.trim() || !prompt.trim() || saving}
              onClick={() => void handleSave()}
            >
              {saving ? "Сохраняю…" : "Сохранить сценарий"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
