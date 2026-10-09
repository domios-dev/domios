import type { RefObject } from "react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { DetailContent } from "../canvasRender/DetailContent";
import type { SectionNavArgs } from "../nodeRegistry";
import { taskDisplayKey } from "../taskDisplayKey";
import type { Plan, Task } from "../types";
import { competitionForTask } from "./competitionForTask";
import { loopForTask } from "./loopForTask";
import { TaskExecutionBadges } from "./TaskExecutionBadges";
import { SelectionAsk } from "../../shared/SelectionAsk";
import { askOwner, askSubject } from "./PlanSidePanel";

export interface TaskSegmentPanelProps {
  panelId: string;
  tabId: string;
  panelRef: RefObject<HTMLElement | null>;
  plan: Plan | null;
  task: Task | null;
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
  plan,
  task,
  taskRef,
  loading = false,
  api,
  tabs,
  onOpenTab,
  onOpenTask,
  onOpenSection,
}: TaskSegmentPanelProps) {
  const key = taskDisplayKey({ id: taskRef, key: task?.key });
  return (
    <section
      ref={panelRef}
      id={panelId}
      role="tabpanel"
      aria-labelledby={tabId}
      tabIndex={-1}
      data-testid="plan-task-segment"
      data-task-id={task?.id ?? taskRef}
      className="plan-task-segment"
    >
      {task && plan ? (
        <>
          <header className="plan-task-segment__header">
            <span data-testid="plan-task-segment-key" className="plan-task-segment__key">
              {key}
            </span>
            <h2 data-testid="plan-task-segment-title" className="plan-task-segment__title plan-task-selectable">
              {task.name}
            </h2>
            <TaskExecutionBadges
              loop={loopForTask(plan, task.id)}
              competition={competitionForTask(task, tabs)}
              className="plan-detail-header-badges"
            />
          </header>
          <SelectionAsk className="plan-task-segment__body plan-detail-selectable" subject={askSubject(plan, task, key)} owner={askOwner(plan, tabs)} onJump={(id) => onOpenTab?.(id)}>
            <DetailContent
              kind="task"
              planId={plan.id}
              api={api}
              task={task}
              plan={plan}
              tabs={tabs}
              onOpenTab={onOpenTab}
              onOpenTask={onOpenTask}
              onSectionClick={(args: SectionNavArgs) => onOpenSection?.(args.variant === "spec" ? "spec" : "plan")}
            />
          </SelectionAsk>
        </>
      ) : (
        <p data-testid="plan-task-segment-missing" className="plan-task-segment__note">
          {loading ? `Loading task ${key}…` : `Task ${key} is not in this team's plans.`}
        </p>
      )}
    </section>
  );
}
