import { useMemo, useState } from "react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { milestoneColorMap } from "../milestoneColors";
import { resolveTaskExecutionAttempts, attemptTabId, attemptTabTitle } from "../taskExecutorHistory";
import type { Plan, Task } from "../types";
import { laneTurnStatsLabel } from "../taskTurnStats";
import { AttachmentGallery } from "./attachments";
import { ActivityLogRow } from "./ActivityLogRow";
import {
  milestoneAbsoluteTitle,
  milestoneAgeLabel,
  milestoneKindLabel,
  milestoneTimestampLabel,
  resolveMilestoneAuthor,
  taskHasActivity,
  visibleMilestones,
  VISIBLE_MILESTONE_COLLAPSE,
} from "./taskActivityModel";

export interface TaskActivityTimelineProps {
  task: Task;
  plan?: Plan;
  planId?: string;
  api?: ApiConfig;
  tabs?: Record<string, Tab>;
  now?: number;
}

const TASK_ROW_IDS = {
  row: "plan-task-activity-entry",
  kind: "plan-task-activity-kind",
  age: "plan-task-activity-age",
  stamp: "plan-task-activity-timestamp",
  author: "plan-task-activity-author",
};

function AttemptRow({
  attempt,
  index,
  tabs,
}: {
  attempt: ReturnType<typeof resolveTaskExecutionAttempts>[number];
  index: number;
  tabs: Record<string, Tab>;
}) {
  const tabId = attemptTabId(attempt);
  const label = attemptTabTitle(attempt) ?? (tabId ? tabs[tabId]?.title?.trim() : null);
  const bits = [label, attempt.role, attempt.model, attempt.branch, attempt.report_status?.replace(/_/g, " ")]
    .map((v) => v?.trim())
    .filter(Boolean);
  return (
    <div
      className="plan-task-activity-attempt"
      data-testid="plan-task-activity-attempt"
      data-executor-attempt={index + 1}
    >
      {bits.join(" · ")}
      {attempt.lane_turn_stats?.map((stats) => (
        <div
          key={stats.turn}
          data-testid="plan-task-activity-turn-stats"
          data-turn={stats.turn}
        >
          {laneTurnStatsLabel(stats)}
        </div>
      ))}
    </div>
  );
}

function TaskActivityTimelineBody({
  task,
  plan,
  planId,
  api,
  tabs = {},
  now = Date.now(),
}: TaskActivityTimelineProps) {
  const [expanded, setExpanded] = useState(false);
  const attempts = useMemo(() => resolveTaskExecutionAttempts(task, tabs), [task, tabs]);
  const colors = useMemo(() => (plan ? milestoneColorMap(plan) : new Map<string, string>()), [plan]);
  const { visible, hiddenCount } = useMemo(
    () => visibleMilestones(task.milestones, expanded),
    [task.milestones, expanded],
  );

  return (
    <section
      className="ct-activity plan-task-activity"
      data-testid="plan-task-activity"
      aria-label="Task activity"
    >
      <h3 className="ct-activity__title">Activity</h3>
      {attempts.length > 0 && (
        <div className="plan-task-activity-attempts" data-testid="plan-task-activity-attempts">
          <div className="plan-task-activity-subhead">Attempts</div>
          {attempts.map((attempt, index) => (
            <AttemptRow key={attempt.id ?? `${attempt.tab_id ?? "attempt"}-${index}`} attempt={attempt} index={index} tabs={tabs} />
          ))}
        </div>
      )}
      {task.lane?.cleanup_status?.toLowerCase() === "offered" && (
        <div data-testid="plan-task-lane-cleanup-offer">
          <span>Merged lane cleanup is available.</span>
          {task.lane.cleanup_hints?.[0] && <code>{task.lane.cleanup_hints[0]}</code>}
        </div>
      )}
      {task.milestones.length > 0 && (
        <div
          className={`plan-task-activity-timeline${expanded ? " plan-task-activity-timeline--expanded" : ""}`}
          data-testid="plan-task-activity-timeline"
          data-expanded={expanded ? "true" : "false"}
        >
          <div className="plan-task-activity-subhead">Log</div>
          {visible.map((milestone) => {
            const author = resolveMilestoneAuthor(milestone, tabs);
            const kind = milestoneKindLabel(milestone.kind);
            const color = colors.get(milestone.kind);
            return (
              <ActivityLogRow
                key={milestone.id}
                kind={kind}
                color={color}
                age={milestoneAgeLabel(milestone.ts, now)}
                stamp={milestoneTimestampLabel(milestone.ts, now)}
                stampTitle={milestoneAbsoluteTitle(milestone.ts)}
                author={author}
                text={milestone.text}
                testIds={TASK_ROW_IDS}
                data={{ "milestone-id": milestone.id }}
              >
                <AttachmentGallery api={api} planId={planId} attachments={milestone.attachments} />
              </ActivityLogRow>
            );
          })}
          {hiddenCount > 0 && (
            <button
              type="button"
              className="plan-task-activity-show-all"
              data-testid="plan-task-activity-show-all"
              onClick={() => setExpanded(true)}
            >
              Show all {task.milestones.length} strides
            </button>
          )}
          {expanded && task.milestones.length > VISIBLE_MILESTONE_COLLAPSE && (
            <button
              type="button"
              className="plan-task-activity-show-all"
              data-testid="plan-task-activity-show-less"
              onClick={() => setExpanded(false)}
            >
              Show latest {VISIBLE_MILESTONE_COLLAPSE}
            </button>
          )}
        </div>
      )}
    </section>
  );
}

export function TaskActivityTimeline(props: TaskActivityTimelineProps) {
  const tabs = props.tabs ?? {};
  if (!taskHasActivity(props.task, tabs)) return null;
  return <TaskActivityTimelineBody {...props} />;
}
