import { useEffect, useRef, type KeyboardEvent } from "react";
import { Maximize2, X } from "lucide-react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { DetailContent } from "../canvasRender/DetailContent";
import type { PlanSidePanelTarget } from "../planSidePanelStore";
import { findTaskByReference, taskDisplayKey } from "../taskDisplayKey";
import type { PlanTarget } from "../teamViewNavigation";
import type { Plan } from "../types";
import { competitionForTask } from "./competitionForTask";
import { loopForTask } from "./loopForTask";
import { TaskExecutionBadges } from "./TaskExecutionBadges";

export interface PlanSidePanelProps {
  target: PlanSidePanelTarget;
  plan: Plan | null;
  loading?: boolean;
  api?: ApiConfig;
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  onOpenPlanTarget: (target: PlanTarget) => void;
  onClose: () => void;
}

/** The task or plan preview that slides over whatever surface is showing. */
export function PlanSidePanel({
  target,
  plan,
  loading = false,
  api,
  tabs,
  onOpenTab,
  onOpenPlanTarget,
  onClose,
}: PlanSidePanelProps) {
  const panelRef = useRef<HTMLElement | null>(null);
  const taskRef = target.taskId;
  const task = plan && taskRef ? findTaskByReference(plan.tasks, taskRef) : undefined;
  const key = taskRef ? taskDisplayKey({ id: taskRef, key: task?.key }) : null;
  const route = {
    planId: target.planId,
    teamId: plan?.team_id ?? target.teamId,
    canonicalProjectPath: target.canonicalProjectPath,
  };

  useEffect(() => {
    panelRef.current?.focus();
  }, [target]);

  const showFull = () => onOpenPlanTarget({ ...route, taskId: task?.id ?? taskRef, presentation: "full" });
  const openFullPlan = () => onOpenPlanTarget({ ...route, presentation: "full" });
  const openTask = (taskId: string) => onOpenPlanTarget({ ...route, taskId });

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    event.stopPropagation();
    onClose();
  };

  const title = key ? task?.name : plan?.title;
  const subject = key ? `Task ${key}` : "plan";
  const dialogLabel = key ? `Task ${key}${task ? ` ${task.name}` : ""}` : `Plan ${plan?.title ?? ""}`.trim();
  const missing = key
    ? (loading ? `Loading task ${key}…` : `Task ${key} is not available.`)
    : (loading ? "Loading plan…" : "This plan is not available.");

  return (
    <aside
      ref={panelRef}
      data-plan-side-panel=""
      role="dialog"
      aria-modal="false"
      aria-label={dialogLabel}
      tabIndex={-1}
      data-testid="plan-side-panel"
      data-kind={key ? "task" : "plan"}
      data-plan-id={target.planId}
      data-task-id={task?.id ?? taskRef}
      className="plan-side-panel"
      onKeyDown={onKeyDown}
    >
      <header className="plan-side-panel__header">
        {key && (
          <span data-testid="plan-side-panel-key" className="plan-task-segment__key">
            {key}
          </span>
        )}
        <h2 data-testid="plan-side-panel-title" className="plan-side-panel__title plan-task-selectable">
          {title}
        </h2>
        {plan && task && (
          <TaskExecutionBadges
            loop={loopForTask(plan, task.id)}
            competition={competitionForTask(task, tabs)}
            className="plan-detail-header-badges"
          />
        )}
        <button
          type="button"
          data-testid="plan-side-panel-full"
          aria-label={`Open ${subject} in full view`}
          title={`Open ${subject} in full view`}
          className="plan-side-panel__action"
          onClick={showFull}
        >
          <Maximize2 size={14} aria-hidden />
        </button>
        <button
          type="button"
          data-testid="plan-side-panel-close"
          aria-label={`Close ${subject}`}
          title="Close (Esc)"
          className="plan-side-panel__action"
          onClick={onClose}
        >
          <X size={15} aria-hidden />
        </button>
      </header>
      <div data-testid="plan-side-panel-body" className="plan-side-panel__body plan-detail-selectable">
        {plan && (!key || task) ? (
          <DetailContent
            kind={task ? "task" : "plan"}
            planId={plan.id}
            api={api}
            task={task}
            plan={plan}
            tabs={tabs}
            onOpenTab={onOpenTab}
            onOpenTask={openTask}
            onSectionClick={openFullPlan}
          />
        ) : (
          <p data-testid="plan-side-panel-missing" className="plan-task-segment__note">
            {missing}
          </p>
        )}
      </div>
    </aside>
  );
}
