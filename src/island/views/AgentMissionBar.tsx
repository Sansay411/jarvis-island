// Compact live executive strip rendered in PeekView whenever Jarvis is
// actively executing tools or a multi-step `mission_plan` in the background.

import { Icon } from "../../ui/Icon";
import { useAgent } from "../../store/agent";
import { useHub } from "../../store/hub";
import { useIsland } from "../../store/island";
import type { MissionPlanStep } from "../../lib/types";

export function useMissionActive(): boolean {
  const activeTurn = useAgent((s) => s.activeTurn);
  const hubSteps = useHub((s) => s.missionSteps);
  const agentSteps = useAgent((s) => (s as unknown as { planSteps?: MissionPlanStep[] }).planSteps);
  const steps = agentSteps && agentSteps.length > 0 ? agentSteps : hubSteps;
  return Boolean(activeTurn) || steps.length > 0;
}

export function AgentMissionBar() {
  const activeTurn = useAgent((s) => s.activeTurn);
  const cancel = useAgent((s) => s.cancel);
  const runningToolTitle = useAgent((s) => {
    const it = s.items.findLast((i) => i.kind === "tool" && (i.status === "running" || i.status === "awaiting"));
    return it?.kind === "tool" ? it.title : null;
  });
  const awaiting = useAgent((s) => s.items.some((i) => i.kind === "tool" && i.status === "awaiting"));

  const hubSteps = useHub((s) => s.missionSteps);
  const agentSteps = useAgent((s) => (s as unknown as { planSteps?: MissionPlanStep[] }).planSteps);
  const steps = agentSteps && agentSteps.length > 0 ? agentSteps : hubSteps;

  if (!activeTurn && steps.length === 0) return null;

  const doneCount = steps.filter((s) => s.status === "done").length;
  const currentStep =
    steps.find((s) => s.status === "running") ??
    steps.find((s) => s.status === "pending") ??
    steps[steps.length - 1];

  const headline = awaiting
    ? `Ждёт подтверждения: ${runningToolTitle ?? "действие"}`
    : currentStep
      ? currentStep.title
      : runningToolTitle ?? "Джарвис выполняет задачу…";

  return (
    <div className={`mission-strip${awaiting ? " is-awaiting" : ""}`}>
      <span className="job-ring" aria-hidden="true" />
      {steps.length > 0 && (
        <span className="mission-step-badge">
          {Math.min(steps.length, doneCount + (activeTurn ? 1 : 0))}/{steps.length}
        </span>
      )}
      <button
        type="button"
        className="mission-headline"
        title="Открыть рабочее пространство Джарвиса"
        onClick={() => useIsland.getState().set({ workspaceOpen: true })}
      >
        {headline}
      </button>
      {activeTurn && (
        <button
          type="button"
          className="icon-btn icon-btn-xs"
          aria-label="Остановить выполнение"
          title="Остановить"
          onClick={() => void cancel()}
        >
          <Icon name="stop" size={11} />
        </button>
      )}
    </div>
  );
}
