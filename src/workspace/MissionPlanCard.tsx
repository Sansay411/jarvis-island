// Live multi-step execution plan card displayed in the Workspace when the
// autonomous agent publishes `plan_updated` events via `mission_plan`.

import { useState } from "react";
import { useAgent, type PlanStep } from "../store/agent";
import { WsIcon } from "./WorkspaceIcons";

export function MissionPlanCard({ steps: propSteps }: { steps?: PlanStep[] }) {
  const storeSteps = useAgent((s) => s.planSteps);
  const clearPlan = useAgent((s) => s.clearPlan);
  const steps = propSteps ?? storeSteps;
  const [collapsed, setCollapsed] = useState(false);

  if (!steps || steps.length === 0) return null;

  const total = steps.length;
  const doneCount = steps.filter((s) => s.status === "done").length;
  const runningIdx = steps.findIndex((s) => s.status === "running");
  const failedIdx = steps.findIndex((s) => s.status === "failed");
  const allDone = doneCount === total && total > 0;
  const hasFailed = failedIdx >= 0;
  const isRunning = runningIdx >= 0;

  const stepNumber = isRunning
    ? runningIdx + 1
    : hasFailed
      ? failedIdx + 1
      : Math.min(total, doneCount + 1);

  const statusText = hasFailed
    ? `Шаг ${failedIdx + 1} из ${total} · Ошибка`
    : allDone
      ? `Выполнено ${total} из ${total}`
      : isRunning
        ? `Шаг ${stepNumber} из ${total} · Выполняется`
        : `Шаг ${stepNumber} из ${total} · В очереди`;

  const progressPct = Math.min(
    100,
    Math.round(((doneCount + (isRunning ? 0.45 : 0)) / Math.max(1, total)) * 100),
  );

  return (
    <section
      className={`mission-plan${allDone ? " is-done" : hasFailed ? " is-failed" : isRunning ? " is-running" : ""}`}
      aria-label="План выполнения задачи"
    >
      <div className="mission-plan-head">
        <button
          type="button"
          className="mission-plan-toggle"
          onClick={() => setCollapsed((c) => !c)}
          aria-expanded={!collapsed}
        >
          <span className="mission-plan-glyph">
            {isRunning ? (
              <span className="plan-ring-live" aria-hidden="true" />
            ) : allDone ? (
              <span className="plan-glyph-done" aria-hidden="true">
                <WsIcon name="check" size={12} />
              </span>
            ) : hasFailed ? (
              <span className="plan-glyph-failed" aria-hidden="true">!</span>
            ) : (
              <WsIcon name="plan" size={14} />
            )}
          </span>
          <span className="mission-plan-title">План миссии</span>
          <span className="mission-plan-sep">·</span>
          <span className="mission-plan-meta">{statusText}</span>
          <span className={`mission-plan-chevron${collapsed ? " is-collapsed" : ""}`}>
            <WsIcon name="chevronDown" size={13} />
          </span>
        </button>

        {allDone && (
          <button
            type="button"
            className="mission-plan-clear"
            onClick={() => clearPlan()}
            title="Скрыть завершённый план"
            aria-label="Скрыть завершённый план"
          >
            <WsIcon name="close" size={12} />
          </button>
        )}
      </div>

      <div className="mission-plan-bar" aria-hidden="true">
        <span style={{ width: `${progressPct}%` }} />
      </div>

      {!collapsed && (
        <ol className="mission-plan-steps">
          {steps.map((st, idx) => (
            <li key={st.id || idx} className={`mission-step is-${st.status}`}>
              <span className="mission-step-badge" aria-hidden="true">
                {st.status === "running" ? (
                  <span className="plan-ring-live is-sm" />
                ) : st.status === "done" ? (
                  <span className="plan-check-done">
                    <WsIcon name="check" size={11} />
                  </span>
                ) : st.status === "failed" ? (
                  <span className="plan-dot-failed">!</span>
                ) : (
                  <span className="plan-step-index">{idx + 1}</span>
                )}
              </span>
              <span className="mission-step-title">{st.title}</span>
              <span className="mission-step-state">
                {st.status === "running"
                  ? "Выполняется"
                  : st.status === "done"
                    ? "Готово"
                    : st.status === "failed"
                      ? "Ошибка"
                      : `Шаг ${idx + 1}`}
              </span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
