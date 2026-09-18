import { useMemo, useState } from "react";
import type { ApiConfig, Tab } from "../../../lib/types";
import { milestoneColorMap } from "../milestoneColors";
import { resolveTaskExecutionAttempts, attemptTabId, attemptTabTitle } from "../taskExecutorHistory";
import type { Plan, Task } from "../types";
import { AttachmentGallery } from "./attachments";
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

function KindChip({ label, color }: { label: string; color?: string }) {
  return (
    <span className="plan-task-activity-kind" data-testid="plan-task-activity-kind">
      {color && <span className="plan-task-activity-kind-dot" style={{ background: color }} />}
      {label}
    </span>
  );
}

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
      className="plan-task-activity"
      data-testid="plan-task-activity"
      aria-label="Task activity"
    >
      <div className="plan-task-activity-label">Activity</div>
      {attempts.length > 0 && (
        <div className="plan-task-activity-attempts" data-testid="plan-task-activity-attempts">
          <div className="plan-task-activity-subhead">Execution attempts</div>
          {attempts.map((attempt, index) => (
            <AttemptRow key={attempt.id ?? `${attempt.tab_id ?? "attempt"}-${index}`} attempt={attempt} index={index} tabs={tabs} />
          ))}
        </div>
      )}
      {task.milestones.length > 0 && (
        <div
          className={`plan-task-activity-timeline${expanded ? " plan-task-activity-timeline--expanded" : ""}`}
          data-testid="plan-task-activity-timeline"
          data-expanded={expanded ? "true" : "false"}
        >
          <div className="plan-task-activity-subhead">Strides</div>
          {visible.map((milestone) => {
            const author = resolveMilestoneAuthor(milestone, tabs);
            const kind = milestoneKindLabel(milestone.kind);
            const color = colors.get(milestone.kind);
            return (
              <article
                key={milestone.id}
                className="plan-task-activity-entry"
                data-testid="plan-task-activity-entry"
                data-milestone-id={milestone.id}
              >
                <div className="plan-task-activity-entry-meta">
                  <KindChip label={kind} color={color} />
                  <span
                    className="plan-task-activity-timestamp"
                    data-testid="plan-task-activity-timestamp"
                    title={milestoneAbsoluteTitle(milestone.ts)}
                  >
                    {milestoneTimestampLabel(milestone.ts, now)}
                  </span>
                  <span
                    className="plan-task-activity-age"
                    data-testid="plan-task-activity-age"
                  >
                    {milestoneAgeLabel(milestone.ts, now)}
                  </span>
                  {author && (
                    <span className="plan-task-activity-author" data-testid="plan-task-activity-author">
                      {author}
                    </span>
                  )}
                </div>
                <div className="plan-task-activity-text">{milestone.text}</div>
                <AttachmentGallery api={api} planId={planId} attachments={milestone.attachments} />
              </article>
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
