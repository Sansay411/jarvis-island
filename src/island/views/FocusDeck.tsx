// Needt-inspired Focus & Tasks experience (Reference 6: `b840e554.jpg`).
// 1. `FocusHoverBanner`: sleek compact hover card (`SVG ring 18`, `Джарвис · Фокус ●`,
//    bold task title, slim progress bar, `17:55 осталось · до 14:38`, `Далее: ...`,
//    tactile circular `[⏸]` and cobalt `[✓]` buttons).
// 2. `FocusDeck`: expanded two-column Bento (`FocusHeroCard` on left + color-ring
//    task queue & inline `+ Добавить задачу` composer on right).

import { useMemo, useState, type FormEvent } from "react";
import { Icon } from "../../ui/Icon";
import { emote } from "../../store/emote";
import { islandInputHandlers } from "../../store/island";
import { useHub } from "../../store/hub";
import { usePrefs } from "../../store/prefs";
import type { TaskColor, TaskItem } from "../../lib/types";

const PRESETS = [15, 25, 45, 60] as const;
const RING_COLORS: TaskColor[] = ["blue", "amber", "coral", "emerald", "violet"];

function fmtRemaining(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  const m = Math.floor(s / 60);
  const r = String(s % 60).padStart(2, "0");
  return `${m}:${r}`;
}

function fmtEndsAt(endsAt: string | null, remainingSec: number): string {
  const dt = endsAt ? new Date(endsAt) : new Date(Date.now() + remainingSec * 1000);
  if (Number.isNaN(dt.getTime())) return "";
  return dt.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
}

function JarvisMiniMark() {
  return (
    <span className="jarvis-mini-mark" aria-hidden="true">
      J
    </span>
  );
}

function FocusRingSvg({
  size,
  stroke,
  fraction,
  centerLabel,
  paused,
}: {
  size: number;
  stroke: number;
  fraction: number;
  centerLabel: string | number;
  paused?: boolean;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const clamped = Math.min(1, Math.max(0, fraction));
  return (
    <div className={`focus-ring-wrap${paused ? " is-paused" : ""}`} style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="focus-ring-svg" aria-hidden="true">
        <circle cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} className="focus-ring-track" />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          strokeWidth={stroke}
          strokeDasharray={c}
          strokeDashoffset={c * (1 - clamped)}
          className="focus-ring-arc"
        />
      </svg>
      <span className="focus-ring-num">{centerLabel}</span>
    </div>
  );
}

/** Reference 6 TOP ("Hover the notch. See your focus.") */
export function FocusHoverBanner({ onExpand }: { onExpand: () => void }) {
  const focus = useHub((s) => s.focus);
  const tasks = useHub((s) => s.tasks);
  const pauseToggle = useHub((s) => s.focusPauseToggle);
  const complete = useHub((s) => s.focusComplete);

  const nextTask = useMemo(
    () =>
      tasks.find((t) => !t.completed && t.id !== focus.taskId) ??
      tasks.find((t) => !t.completed) ??
      null,
    [tasks, focus.taskId],
  );

  const total = Math.max(1, focus.totalSec);
  const rem = Math.max(0, focus.remainingSec);
  const ringFrac = rem / total;
  const elapsedFrac = Math.min(1, Math.max(0.04, 1 - rem / total));
  const minsCeil = Math.max(1, Math.ceil(rem / 60));
  const endsLabel = fmtEndsAt(focus.endsAt, rem);

  return (
    <div
      className="focus-banner"
      onClick={onExpand}
      role="region"
      aria-label="Текущая сессия фокуса"
    >
      <FocusRingSvg size={48} stroke={3.8} fraction={ringFrac} centerLabel={minsCeil} paused={focus.paused} />

      <div className="focus-banner-body">
        <div className="focus-eyebrow">
          <JarvisMiniMark />
          <span>Джарвис · Фокус</span>
          <span className={`focus-live-dot${focus.paused ? " is-paused" : ""}`} aria-hidden="true" />
        </div>

        <div className="focus-banner-title" title={focus.title}>
          {focus.title || "Глубокая работа"}
        </div>

        <div className="focus-banner-bar" aria-hidden="true">
          <span style={{ width: `${(elapsedFrac * 100).toFixed(1)}%` }} />
        </div>

        <div className="focus-banner-meta">
          <span>
            {fmtRemaining(rem)} осталось
            {focus.paused ? " · на паузе" : endsLabel ? ` · до ${endsLabel}` : ""}
          </span>
          <span className="focus-banner-next">
            {nextTask
              ? `Далее: ${nextTask.title}${nextTask.scheduledTime ? ` ${nextTask.scheduledTime}` : ""}`
              : `Сегодня: ${focus.minutesToday} мин в фокусе`}
          </span>
        </div>
      </div>

      <div className="focus-banner-actions" onClick={(e) => e.stopPropagation()}>
        <button
          type="button"
          className="focus-circle-btn is-secondary"
          aria-label={focus.paused ? "Продолжить фокус" : "Пауза фокуса"}
          title={focus.paused ? "Продолжить" : "Пауза"}
          onClick={() => void pauseToggle()}
        >
          <Icon name={focus.paused ? "play" : "pause"} size={14} />
        </button>
        <button
          type="button"
          className="focus-circle-btn is-cobalt"
          aria-label="Завершить фокус"
          title="Готово"
          onClick={() => {
            emote("star");
            void complete();
          }}
        >
          <Icon name="check" size={15} />
        </button>
      </div>
    </div>
  );
}

/** Reference 6 BOTTOM ("CLICK" Expanded Focus Card + Task Queue) */
export function FocusDeck() {
  const focus = useHub((s) => s.focus);
  const tasks = useHub((s) => s.tasks);
  const focusStart = useHub((s) => s.focusStart);
  const focusPauseToggle = useHub((s) => s.focusPauseToggle);
  const focusComplete = useHub((s) => s.focusComplete);
  const focusCancel = useHub((s) => s.focusCancel);
  const taskAdd = useHub((s) => s.taskAdd);
  const taskToggle = useHub((s) => s.taskToggle);
  const taskRemove = useHub((s) => s.taskRemove);
  const shortcut = usePrefs((p) => p.workspaceShortcut);

  const [presetMin, setPresetMin] = useState<number>(25);
  const [customTitle, setCustomTitle] = useState<string>("");
  const [newTaskText, setNewTaskText] = useState<string>("");
  const [ringColorIdx, setRingColorIdx] = useState<number>(0);

  const inputProps = islandInputHandlers();

  const sortedTasks = useMemo(() => {
    return [...tasks].sort((a, b) => {
      if (a.completed !== b.completed) return a.completed ? 1 : -1;
      return (a.scheduledTime ?? "99:99").localeCompare(b.scheduledTime ?? "99:99");
    });
  }, [tasks]);

  const openTasks = useMemo(() => sortedTasks.filter((t) => !t.completed), [sortedTasks]);
  const defaultFocusTask: TaskItem | undefined = openTasks[0];

  const startFromHero = () => {
    const chosenTitle = customTitle.trim() || defaultFocusTask?.title || "Глубокая работа";
    const linkedId = !customTitle.trim() && defaultFocusTask ? defaultFocusTask.id : null;
    emote("determined");
    void focusStart(chosenTitle, presetMin, linkedId);
  };

  const startForTask = (t: TaskItem) => {
    emote("determined");
    void focusStart(t.title, presetMin, t.id);
  };

  const submitTask = async (e: FormEvent) => {
    e.preventDefault();
    if (!newTaskText.trim()) return;
    const color = RING_COLORS[ringColorIdx % RING_COLORS.length]!;
    await taskAdd(newTaskText, { color });
    setNewTaskText("");
    setRingColorIdx((i) => (i + 1) % RING_COLORS.length);
  };

  const totalSec = Math.max(1, focus.totalSec);
  const remSec = Math.max(0, focus.remainingSec);
  const activeMins = Math.max(1, Math.ceil(remSec / 60));
  const endsLabel = fmtEndsAt(focus.endsAt, remSec);
  const activeColor = RING_COLORS[ringColorIdx % RING_COLORS.length]!;

  return (
    <div className="focus-deck">
      {/* Left Elevated Bento Card */}
      <div className={`focus-card${focus.active ? " is-active" : ""}`}>
        <div className="focus-card-top">
          <FocusRingSvg
            size={56}
            stroke={4.2}
            fraction={focus.active ? remSec / totalSec : 1}
            centerLabel={focus.active ? activeMins : presetMin}
            paused={focus.active && focus.paused}
          />
          {focus.active && (
            <button
              type="button"
              className="focus-cancel-btn"
              aria-label="Сбросить сессию фокуса"
              title="Отменить сессию"
              onClick={() => void focusCancel()}
            >
              <Icon name="close" size={12} />
            </button>
          )}
        </div>

        <div className="focus-card-mid">
          <div className="focus-eyebrow">
            <JarvisMiniMark />
            <span>Фокус</span>
            <span
              className={`focus-live-dot${!focus.active ? " is-idle" : focus.paused ? " is-paused" : ""}`}
              aria-hidden="true"
            />
          </div>

          {focus.active ? (
            <>
              <div className="focus-card-title" title={focus.title}>
                {focus.title}
              </div>
              <div className="focus-card-sub">
                {fmtRemaining(remSec)} осталось
                {focus.paused ? " · пауза" : endsLabel ? ` · до ${endsLabel}` : ""}
              </div>
            </>
          ) : (
            <>
              <input
                type="text"
                className="focus-title-input"
                value={customTitle}
                onChange={(e) => setCustomTitle(e.target.value)}
                placeholder={defaultFocusTask ? defaultFocusTask.title : "Над чем сфокусируемся?"}
                aria-label="Тема фокуса"
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    startFromHero();
                  }
                }}
                {...inputProps}
              />
              <div className="focus-presets" role="group" aria-label="Длительность фокуса">
                {PRESETS.map((m) => (
                  <button
                    key={m}
                    type="button"
                    className={`focus-preset-pill${presetMin === m ? " is-selected" : ""}`}
                    onClick={() => setPresetMin(m)}
                  >
                    {m}м
                  </button>
                ))}
              </div>
            </>
          )}
        </div>

        <div className="focus-card-foot">
          {focus.active ? (
            <div className="focus-pill-pair">
              <button type="button" className="focus-pill-btn is-dark" onClick={() => void focusPauseToggle()}>
                <Icon name={focus.paused ? "play" : "pause"} size={12} />
                <span>{focus.paused ? "Дальше" : "Пауза"}</span>
              </button>
              <button
                type="button"
                className="focus-pill-btn is-cobalt"
                onClick={() => {
                  emote("star");
                  void focusComplete();
                }}
              >
                <Icon name="check" size={13} />
                <span>Готово</span>
              </button>
            </div>
          ) : (
            <button type="button" className="focus-pill-btn is-cobalt is-wide" onClick={startFromHero}>
              <Icon name="play" size={12} />
              <span>Начать · {presetMin} мин</span>
            </button>
          )}
        </div>
      </div>

      {/* Right Task Queue Column */}
      <div className="focus-queue">
        <div className="focus-queue-head">
          <span className="focus-queue-label">Далее</span>
          <span className="focus-queue-stats">
            {openTasks.length} осталось · {focus.minutesToday} мин сегодня
          </span>
        </div>

        <div className="focus-task-list scroll">
          {sortedTasks.length === 0 ? (
            <div className="focus-empty">
              Добавьте ключевые задачи на день — нажмите на любую, чтобы запустить таймер фокуса.
            </div>
          ) : (
            sortedTasks.map((t) => (
              <div
                key={t.id}
                className={`focus-task-row${t.completed ? " is-done" : ""}${focus.active && focus.taskId === t.id ? " is-current" : ""}`}
              >
                <button
                  type="button"
                  className={`task-ring ring-${t.color || "blue"}${t.completed ? " is-checked" : ""}`}
                  aria-label={t.completed ? "Снять отметку" : "Выполнить задачу"}
                  onClick={() => void taskToggle(t.id)}
                >
                  {t.completed && <Icon name="check" size={10} />}
                </button>

                <button
                  type="button"
                  className="focus-task-title"
                  title={t.completed ? t.title : `Запустить фокус: ${t.title}`}
                  onClick={() => {
                    if (!t.completed) startForTask(t);
                  }}
                >
                  {t.title}
                </button>

                {t.scheduledTime && <span className="focus-task-time">{t.scheduledTime}</span>}

                <button
                  type="button"
                  className="focus-task-del"
                  aria-label="Удалить задачу"
                  onClick={() => void taskRemove(t.id)}
                >
                  <Icon name="close" size={11} />
                </button>
              </div>
            ))
          )}
        </div>

        <form className="focus-add-bar" onSubmit={(e) => void submitTask(e)}>
          <button
            type="button"
            className={`focus-color-pick ring-${activeColor}`}
            title="Сменить цвет метки"
            aria-label="Сменить цвет задачи"
            onClick={() => setRingColorIdx((i) => (i + 1) % RING_COLORS.length)}
          >
            <Icon name="plus" size={10} />
          </button>
          <input
            type="text"
            value={newTaskText}
            onChange={(e) => setNewTaskText(e.target.value)}
            placeholder="Добавить задачу (напр. Отчёт 15:00)"
            aria-label="Добавить задачу"
            {...inputProps}
          />
          {shortcut && <kbd className="focus-kbd">{shortcut.replaceAll("+", " ")}</kbd>}
        </form>
      </div>
    </div>
  );
}
