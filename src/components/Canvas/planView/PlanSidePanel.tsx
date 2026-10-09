import { SidePanelFrame } from "./SidePanelFrame";
import { useEffect, useState, type RefObject } from "react";
import { Maximize2 } from "lucide-react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { DetailContent } from "../canvasRender/DetailContent";
import type { PlanSidePanelTarget } from "../planSidePanelStore";
import { findTaskByReference, taskDisplayKey } from "../taskDisplayKey";
import type { PlanTarget } from "../teamViewNavigation";
import type { Plan, Task } from "../types";
import { SelectionAsk, type SelectionAskOwner } from "../../shared/SelectionAsk";
import { resolvePlanAskOwner } from "./planAskModel";
import type { PresenceState } from "../usePresence";
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
  state: PresenceState;
  panelRef: RefObject<HTMLElement | null>;
}

export function askSubject(plan: Plan, task: Task | undefined, key: string | null): string {
  const planRef = `plan "${plan.title}" (${plan.id})`;
  return task ? `task ${key ?? task.id} "${task.name}" in ${planRef}` : planRef;
}

export function askOwner(plan: Plan, tabs: Record<string, Tab> | undefined): SelectionAskOwner | null {
  const owner = resolvePlanAskOwner(plan, tabs);
  return owner.status === "ready" ? { tabId: owner.tabId, name: owner.name } : null;
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
  state,
  panelRef,
}: PlanSidePanelProps) {
  useEffect(() => { panelRef.current?.focus(); }, [panelRef, target]);
  const contentKey = `${target.planId}/${target.taskId ?? ""}`;
  const [content, setContent] = useState({ key: contentKey, replaced: false });
  if (content.key !== contentKey) setContent({ key: contentKey, replaced: true });
  const taskRef = target.taskId;
  const task = plan && taskRef ? findTaskByReference(plan.tasks, taskRef) : undefined;
  const key = taskRef ? taskDisplayKey({ id: taskRef, key: task?.key }) : null;
  const route = {
    returnTo: target.returnTo,
    planId: target.planId,
    teamId: plan?.team_id ?? target.teamId,
    canonicalProjectPath: target.canonicalProjectPath,
  };

  const showFull = () => onOpenPlanTarget({ ...route, taskId: task?.id ?? taskRef, presentation: "full" });
  const openFullPlan = () => onOpenPlanTarget({ ...route, presentation: "full" });
  const openTask = (taskId: string) => onOpenPlanTarget({ ...route, taskId });

  const title = key ? task?.name : plan?.title;
  const subject = key ? `Task ${key}` : "plan";
  const dialogLabel = key ? `Task ${key}${task ? ` ${task.name}` : ""}` : `Plan ${plan?.title ?? ""}`.trim();
  const missing = key
    ? (loading ? `Loading task ${key}…` : `Task ${key} is not available.`)
    : (loading ? "Loading plan…" : "This plan is not available.");

  return (
    <SidePanelFrame panelRef={panelRef} state={state} label={dialogLabel} closeLabel={`Close ${subject}`} onClose={onClose} contentKey={contentKey} replaced={content.replaced}
      data-testid="plan-side-panel" data-kind={key ? "task" : "plan"} data-plan-id={target.planId} data-task-id={task?.id ?? taskRef}
      title={<>
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
      </>}
    >
        {plan && (!key || task) ? (
          <SelectionAsk subject={askSubject(plan, task, key)} owner={askOwner(plan, tabs)} onJump={(tabId) => onOpenTab?.(tabId)}>
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
          </SelectionAsk>
        ) : (
          <p data-testid="plan-side-panel-missing" className="plan-task-segment__note">
            {missing}
          </p>
        )}
    </SidePanelFrame>
  );
}
