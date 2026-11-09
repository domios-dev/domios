import type { PlanTaskView as Task } from "../planProjection";
import type { PlanView as Plan } from "../planProjection";
import { TaskHistoryControls, type TaskHistoryController } from "./TaskHistoryControls";
import { SidePanelFrame } from "./SidePanelFrame";
import { taskDetailMatchesReference } from "../usePlanTaskDetail";
import { useEffect, useState, type RefObject } from "react";
import { Maximize2 } from "lucide-react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { InlineErrorNotice } from "../../shared/ErrorNotice";
import { DetailContent } from "../canvasRender/DetailContent";
import type { PlanSidePanelTarget } from "../planSidePanelStore";
import { findTaskByReference, taskDisplayKey } from "../taskDisplayKey";
import type { PlanTarget } from "../teamViewNavigation";
import type { PlanTaskDetail, PlanTaskDetailContext } from "../types";
import { SelectionAsk, type SelectionAskOwner } from "../../shared/SelectionAsk";
import { Loader } from "../../shared/Loader";
import { resolvePlanAskOwner } from "./planAskModel";
import type { PresenceState } from "../usePresence";
import { competitionForTask } from "./competitionForTask";
import { loopForTask } from "./loopForTask";
import { TaskExecutionBadges } from "./TaskExecutionBadges";

export interface PlanSidePanelProps {
  target: PlanSidePanelTarget;
  plan: Plan | null;
  taskDetail?: PlanTaskDetail | null;
  historyController?: TaskHistoryController;
  taskLoading?: boolean;
  taskError?: string | null;
  onRetryTask?: () => Promise<void>;
  loading?: boolean;
  api?: ApiConfig;
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  onOpenPlanTarget: (target: PlanTarget) => void;
  onClose: () => void;
  state: PresenceState;
  panelRef: RefObject<HTMLElement | null>;
}

export function askSubject(plan: Plan | PlanTaskDetailContext, task: Task | undefined, key: string | null): string {
  const planId = "plan_id" in plan ? plan.plan_id : plan.id;
  const planRef = `plan "${plan.title}" (${planId})`;
  return task ? `task ${key ?? task.id} "${task.name}" in ${planRef}` : planRef;
}

export function askOwner(plan: Pick<Plan, "owner">, tabs: Record<string, Tab> | undefined): SelectionAskOwner | null {
  const owner = resolvePlanAskOwner(plan, tabs);
  return owner.status === "ready" ? { tabId: owner.tabId, name: owner.name } : null;
}

/** The task or plan preview that slides over whatever surface is showing. */
export function PlanSidePanel({
  target,
  plan,
  taskDetail = null,
  historyController,
  taskLoading = false,
  taskError = null,
  onRetryTask,
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
  const detailMatches = Boolean(
    taskRef
    && taskDetail
    && taskDetailMatchesReference(taskDetail, target.planId, taskRef),
  );
  const task = detailMatches
    ? taskDetail!.task
    : !api && plan && taskRef
      ? findTaskByReference(plan.tasks, taskRef)
      : undefined;
  const taskContext = detailMatches ? taskDetail!.context : undefined;
  const key = taskRef ? taskDisplayKey(task ?? { id: taskRef }) : null;
  const route = {
    returnTo: target.returnTo,
    planId: target.planId,
    teamId: taskContext?.team_id ?? plan?.team_id ?? target.teamId,
    canonicalProjectPath: target.canonicalProjectPath,
  };

  const showFull = () => onOpenPlanTarget({ ...route, taskId: task?.id ?? taskRef, presentation: "full" });
  const openFullPlan = () => onOpenPlanTarget({ ...route, presentation: "full" });
  const openTask = (taskId: string) => onOpenPlanTarget({ ...route, taskId, nestedFromPlan: !key || target.nestedFromPlan });
  const backToPlan = key && target.nestedFromPlan ? () => onOpenPlanTarget(route) : undefined;

  const title = key ? task?.name : plan?.title;
  const subject = key ? `Task ${key}` : "plan";
  const dialogLabel = key ? `Task ${key}${task ? ` ${task.name}` : ""}` : `Plan ${plan?.title ?? ""}`.trim();
  const missing = key
    ? (taskLoading ? `Loading task ${key}…` : `Task ${key} is not available.`)
    : (loading ? "Loading plan…" : "This plan is not available.");

  return (
    <SidePanelFrame panelRef={panelRef} state={state} label={dialogLabel} onBack={backToPlan} backLabel="plan" onClose={onClose} contentKey={contentKey} replaced={content.replaced}
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
        {task && (taskContext || plan) && (
          <TaskExecutionBadges
            loop={detailMatches ? taskDetail?.loop_badge ?? null : plan ? loopForTask(plan, task.id) : null}
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
        {task && (taskContext || plan) ? (
          <SelectionAsk subject={askSubject(taskContext ?? plan!, task, key)} owner={askOwner(taskContext ?? plan!, tabs)} onJump={(tabId) => onOpenTab?.(tabId)}>
            <DetailContent
              kind="task"
              planId={target.planId}
              api={api}
              task={task}
              taskContext={taskContext}
              plan={taskContext ? undefined : plan ?? undefined}
              tabs={tabs}
              onOpenTab={onOpenTab}
              onOpenTask={openTask}
              onSectionClick={openFullPlan}
            />
            {detailMatches && taskDetail && <TaskHistoryControls detail={taskDetail} controller={historyController} />}
          </SelectionAsk>
        ) : !key && plan ? (
          <SelectionAsk subject={askSubject(plan, undefined, null)} owner={askOwner(plan, tabs)} onJump={(tabId) => onOpenTab?.(tabId)}>
            <DetailContent
              kind="plan"
              planId={plan.id}
              api={api}
              plan={plan}
              tabs={tabs}
              onOpenTab={onOpenTab}
              onOpenTask={openTask}
              onSectionClick={openFullPlan}
            />
          </SelectionAsk>
        ) : key && taskError ? (
          <div data-testid="plan-side-panel-task-error" className="flex flex-col items-center gap-3 px-4 py-6 text-center">
            <InlineErrorNotice error={taskError} operation="loading task details" />
            {onRetryTask && <button type="button" onClick={() => void onRetryTask()} className="rounded border px-3 py-1 text-xs">Retry</button>}
          </div>
        ) : (key ? taskLoading : loading) ? (
          <Loader variant="compact" label={missing} />
        ) : (
          <p data-testid="plan-side-panel-missing" className="plan-task-segment__note">
            {missing}
          </p>
        )}
    </SidePanelFrame>
  );
}
