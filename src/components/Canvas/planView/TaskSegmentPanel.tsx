import type { PlanView as Plan } from "../planProjection";
import { TaskHistoryControls, type TaskHistoryController } from "./TaskHistoryControls";
import type { RefObject } from "react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { DetailContent } from "../canvasRender/DetailContent";
import type { SectionNavArgs } from "../nodeRegistry";
import { taskDisplayKey } from "../taskDisplayKey";
import type { PlanTaskDetail } from "../types";
import { taskDetailMatchesReference } from "../usePlanTaskDetail";
import { competitionForTask } from "./competitionForTask";
import { loopForTask } from "./loopForTask";
import { TaskExecutionBadges } from "./TaskExecutionBadges";
import { SelectionAsk } from "../../shared/SelectionAsk";
import { askOwner, askSubject } from "./PlanSidePanel";

export interface TaskSegmentPanelProps {
  panelId: string;
  tabId: string;
  panelRef: RefObject<HTMLElement | null>;
  detail: PlanTaskDetail | null;
  historyController?: TaskHistoryController;
  fallbackPlan?: Plan | null;
  taskRef: string;
  loading?: boolean;
  api?: ApiConfig;
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  onOpenTask?: (taskId: string) => void;
  onOpenSection?: (variant: "plan" | "spec") => void;
}

/** The full task detail as the temporary team-view segment's panel. */
export function TaskSegmentPanel({
  panelId,
  tabId,
  panelRef,
  detail,
  historyController,
  fallbackPlan = null,
  taskRef,
  loading = false,
  api,
  tabs,
  onOpenTab,
  onOpenTask,
  onOpenSection,
}: TaskSegmentPanelProps) {
  const cachedTask = !api && fallbackPlan ? fallbackPlan.tasks.find((candidate) => candidate.id === taskRef) : undefined;
  const context = detail?.context;
  const exactDetail = detail && context && taskDetailMatchesReference(detail, context.plan_id, taskRef);
  const exactCachedTask = !api && cachedTask?.id === taskRef;
  const key = taskDisplayKey(exactDetail ? detail!.task : exactCachedTask ? cachedTask! : { id: taskRef });
  const displayTask = exactDetail ? detail!.task : exactCachedTask ? cachedTask : undefined;
  const displayPlan = exactDetail ? undefined : exactCachedTask ? fallbackPlan : undefined;
  return (
    <section
      ref={panelRef}
      id={panelId}
      role="tabpanel"
      aria-labelledby={tabId}
      tabIndex={-1}
      data-testid="plan-task-segment"
      data-task-id={displayTask?.id ?? taskRef}
      className="plan-task-segment"
    >
      {displayTask && (exactDetail ? context : displayPlan) ? (
        <>
          <header className="plan-task-segment__header">
            <span data-testid="plan-task-segment-key" className="plan-task-segment__key">
              {key}
            </span>
            <h2 data-testid="plan-task-segment-title" className="plan-task-segment__title plan-task-selectable">
              {displayTask.name}
            </h2>
            <TaskExecutionBadges
              loop={exactDetail ? detail!.loop_badge ?? null : displayPlan ? loopForTask(displayPlan, displayTask.id) : null}
              competition={competitionForTask(displayTask, tabs)}
              className="plan-detail-header-badges"
            />
          </header>
          <SelectionAsk className="plan-task-segment__body plan-detail-selectable" subject={askSubject(exactDetail ? context! : displayPlan!, displayTask, key)} owner={askOwner(exactDetail ? context! : displayPlan!, tabs)} onJump={(id) => onOpenTab?.(id)}>
            <DetailContent
              kind="task"
              planId={context?.plan_id ?? displayPlan?.id ?? ""}
              api={api}
              task={displayTask}
              taskContext={exactDetail ? context : undefined}
              plan={displayPlan ?? undefined}
              tabs={tabs}
              onOpenTab={onOpenTab}
              onOpenTask={onOpenTask}
              onSectionClick={(args: SectionNavArgs) => onOpenSection?.(args.variant === "spec" ? "spec" : "plan")}
            />
          </SelectionAsk>
          {exactDetail && <TaskHistoryControls detail={detail!} controller={historyController} />}
        </>
      ) : (
        <p data-testid="plan-task-segment-missing" className="plan-task-segment__note">
          {loading ? `Loading task ${key}…` : `Task ${key} is not in this team's plans.`}
        </p>
      )}
    </section>
  );
}
