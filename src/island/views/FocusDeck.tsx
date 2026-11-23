// Needt-inspired Focus & Tasks experience (Reference 6: `b840e554.jpg`).
// 1. `FocusHoverBanner`: sleek compact hover card (`SVG ring 18`, `Джарвис · Фокус ●`,
//    bold task title, slim progress bar, `17:55 осталось · до 14:38`, `Далее: ...`,
//    tactile circular `[⏸]` and cobalt `[✓]` buttons).
// 2. `FocusDeck`: expanded two-column Bento (`FocusHeroCard` on left + color-ring
//    task queue & inline `+ Добавить задачу` composer on right).

import { useMemo, useState, type FormEvent } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { FOCUS_PRESETS } from "../motion";
import { FocusDial } from "./FocusDial";
import { Icon } from "../../ui/Icon";
import { emote } from "../../store/emote";
import { islandInputHandlers } from "../../store/island";
import { useHub } from "../../store/hub";
import { focusBlock, useAgenda } from "../../store/agenda";
import { useTick } from "../../store/stopwatch";
import type { TaskItem } from "../../lib/types";

const PRESETS = FOCUS_PRESETS;

/** Digits that flip when the value changes: the old one rises away through a
 *  blur, the new one comes up from below. Runs once per change. */
export function FlipNumber({ value }: { value: number }) {
  const reduced = useReducedMotion();
  return (
    <span className="flip-num">
      <AnimatePresence initial={false} mode="popLayout">
        <motion.span
          key={value}
          className="flip-num-digit"
          initial={reduced ? { opacity: 0 } : { y: 10, opacity: 0, filter: "blur(4px)" }}
          animate={{ y: 0, opacity: 1, filter: "blur(0px)" }}
          exit={reduced ? { opacity: 0 } : { y: -10, opacity: 0, filter: "blur(4px)" }}
          transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
        >
          {value}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}

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

function FocusRingSvg({
  size,
  stroke,
  fraction,
  centerLabel,
  paused,
  live,
  big,
}: {
  size: number;
  stroke: number;
  fraction: number;
  centerLabel: string | number;
  paused?: boolean;
  live?: boolean;
  big?: boolean;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const clamped = Math.min(1, Math.max(0, fraction));
  return (
    <div className={`focus-ring-wrap${paused ? " is-paused" : ""}${live ? " is-live" : ""}${big ? " is-big" : ""}`} style={{ width: size, height: size }}>
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
      <span className="focus-ring-num">{typeof centerLabel === "number" ? <FlipNumber value={centerLabel} /> : centerLabel}</span>
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
      <FocusRingSvg size={48} stroke={3.8} fraction={ringFrac} centerLabel={minsCeil} paused={focus.paused} live={!focus.paused} />

      <div className="focus-banner-body">
        <div className="focus-eyebrow">
          <span className={`focus-live-dot${focus.paused ? " is-paused" : ""}`} aria-hidden="true" />
          <span>{focus.paused ? "Фокус на паузе" : "Фокус"}</span>
        </div>

        <div className="focus-banner-title" title={focus.title}>
          {focus.title || "Глубокая работа"}
        </div>

        <div className="focus-banner-bar" aria-hidden="true">
          <span style={{ width: `${(elapsedFrac * 100).toFixed(1)}%` }} />
        </div>

        <div className="focus-banner-meta">
          <span>
            осталось {fmtRemaining(rem)}{!focus.paused && endsLabel ? ` · до ${endsLabel}` : ""}
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

  const [presetMin, setPresetMin] = useState<number>(25);
  const [burstKey, setBurstKey] = useState(0);
  const events = useAgenda((s) => s.events);
  useTick(30_000, !focus.active);
  const block = focus.active ? null : focusBlock(events, new Date());
  const [customTitle, setCustomTitle] = useState<string>("");
  const [newTaskText, setNewTaskText] = useState<string>("");

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
    await taskAdd(newTaskText);
    setNewTaskText("");
  };

  const totalSec = Math.max(1, focus.totalSec);
  const remSec = Math.max(0, focus.remainingSec);
  const activeMins = Math.max(1, Math.ceil(remSec / 60));
  const endsLabel = fmtEndsAt(focus.endsAt, remSec);

  return (
    <div className={`focus-deck${focus.active ? " is-active" : ""}`}>
      {/* Hero strip: the dial, what we focus on, and the one main control. */}
      <div className="focus-hero">
        <FocusDial
          fraction={focus.active ? remSec / totalSec : presetMin / 60}
          minutes={focus.active ? activeMins : presetMin}
          caption={focus.active && focus.paused ? "пауза" : "мин"}
          live={focus.active && !focus.paused}
          paused={focus.active && focus.paused}
          active={focus.active}
          burstKey={burstKey}
          onSetMinutes={focus.active ? undefined : setPresetMin}
        />

        <div className="focus-hero-mid">
          <div className="focus-eyebrow">
            <span
              className={`focus-live-dot${!focus.active ? " is-idle" : focus.paused ? " is-paused" : ""}`}
              aria-hidden="true"
            />
            <span>{!focus.active ? "Новая сессия" : focus.paused ? "На паузе" : "Идёт фокус"}</span>
            {focus.active && (
              <button type="button" className="focus-cancel-link" onClick={() => void focusCancel()}>
                Отменить
              </button>
            )}
            {block && (
              <button
                type="button"
                className={`focus-cal-chip${block.started ? " is-live" : ""}`}
                title={`Фокус на ${block.focusMin} мин: «${block.event.title}»`}
                onClick={() => {
                  emote("determined");
                  void focusStart(block.event.title, block.focusMin, null);
                }}
              >
                <Icon name="calendar" size={11} />
                <span>{block.event.title}</span>
                <em>{block.started ? `до ${block.event.end.slice(11, 16)}` : `в ${block.event.start.slice(11, 16)}`}</em>
              </button>
            )}
          </div>

          {focus.active ? (
            <>
              <div className="focus-hero-title" title={focus.title}>
                {focus.title}
              </div>
              <div className="focus-card-sub">
                осталось {fmtRemaining(remSec)}{!focus.paused && endsLabel ? ` · до ${endsLabel}` : ""}
              </div>
            </>
          ) : (
            <>
              <input
                type="text"
                className="focus-hero-input"
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
                    {presetMin === m && (
                      <motion.span
                        layoutId="focus-preset-sel"
                        className="focus-preset-sel"
                        transition={{ type: "spring", stiffness: 520, damping: 34 }}
                      />
                    )}
                    <span className="focus-preset-label">{m} мин</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>

        <div className="focus-hero-actions">
          {focus.active ? (
            <>
              <button
                type="button"
                className="focus-round-btn is-dark"
                aria-label={focus.paused ? "Продолжить" : "Пауза"}
                title={focus.paused ? "Продолжить" : "Пауза"}
                onClick={() => void focusPauseToggle()}
              >
                <Icon name={focus.paused ? "play" : "pause"} size={16} />
              </button>
              <button
                type="button"
                className="focus-round-btn is-bone"
                aria-label="Готово"
                title="Готово"
                onClick={() => {
                  emote("star");
                  setBurstKey((k) => k + 1);
                  void focusComplete();
                }}
              >
                <Icon name="check" size={17} />
              </button>
            </>
          ) : (
            <motion.button
              whileHover={{ scale: 1.06 }}
              whileTap={{ scale: 0.92 }}
              type="button"
              className="focus-round-btn is-bone is-big"
              aria-label={`Начать на ${presetMin} мин`}
              title={`Начать на ${presetMin} мин`}
              onClick={startFromHero}
            >
              <Icon name="play" size={18} />
            </motion.button>
          )}
        </div>
      </div>

      {/* Right Task Queue Column */}
      <div className="focus-queue">
        <div className="focus-queue-head">
          <span className="focus-queue-label">Задачи</span>
          <span className="focus-queue-stats">
            {openTasks.length > 0 ? `${openTasks.length} открыто, ` : ""}
            {focus.minutesToday} мин в фокусе сегодня
          </span>
        </div>

        <div className="focus-task-list scroll">
          {sortedTasks.length === 0 ? (
            <div className="focus-empty">
              Задач на сегодня нет. Время в конце строки — «Отчёт 15:00» — поставит задачу в календарь.
            </div>
          ) : (
            <AnimatePresence initial={false}>
            {sortedTasks.map((t, i) => (
              <motion.div
                layout
                key={t.id}
                initial={{ opacity: 0, x: -10 }}
                animate={{ opacity: 1, x: 0, transition: { delay: Math.min(i, 6) * 0.035 } }}
                exit={{ opacity: 0, x: 16, transition: { duration: 0.18 } }}
                transition={{ type: "spring", stiffness: 480, damping: 36 }}
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
              </motion.div>
            ))}
            </AnimatePresence>
          )}
        </div>

        <form className="focus-add-bar" onSubmit={(e) => void submitTask(e)}>
          <span className="focus-add-icon" aria-hidden="true">
            <Icon name="plus" size={12} />
          </span>
          <input
            type="text"
            value={newTaskText}
            onChange={(e) => setNewTaskText(e.target.value)}
            placeholder="Новая задача"
            aria-label="Добавить задачу"
            {...inputProps}
          />
        </form>
      </div>
    </div>
  );
}
