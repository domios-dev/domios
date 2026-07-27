import { useMemo } from "react";
import type { Tab } from "../../../lib/types";
import { milestoneColorMap } from "../milestoneColors";
import type { Plan } from "../types";
import {
  buildPlanRecentActivity,
  milestoneAbsoluteTitle,
  milestoneAgeLabel,
  milestoneKindLabel,
  resolveMilestoneAuthor,
} from "./taskActivityModel";

export interface PlanRecentActivityRailProps {
  plan: Plan;
  tabs?: Record<string, Tab>;
  onOpenTask?: (taskId: string) => void;
  now?: number;
  limit?: number;
}

export function PlanRecentActivityRail({
  plan,
  tabs = {},
  onOpenTask,
  now = Date.now(),
  limit = 10,
}: PlanRecentActivityRailProps) {
  const items = useMemo(() => buildPlanRecentActivity(plan, limit), [plan, limit]);
  const colors = useMemo(() => milestoneColorMap(plan), [plan]);
  if (items.length === 0) return null;

  return (
    <section className="plan-recent-activity" data-testid="plan-recent-activity" aria-label="Recent activity">
      <div className="plan-recent-activity-label">Recent activity</div>
      <div className="plan-recent-activity-rail">
        {items.map(({ taskId, taskLabel, milestone }) => {
          const author = resolveMilestoneAuthor(milestone, tabs);
          const kind = milestoneKindLabel(milestone.kind);
          const color = colors.get(milestone.kind);
          const snippet = milestone.text.replace(/\s+/g, " ").trim().slice(0, 72);
          const RowTag = onOpenTask ? "button" : "div";
          return (
            <RowTag
              key={`${taskId}:${milestone.id}`}
              type={onOpenTask ? "button" : undefined}
              className="plan-recent-activity-item"
              data-testid="plan-recent-activity-item"
              data-task-id={taskId}
              data-milestone-id={milestone.id}
              onClick={onOpenTask ? () => onOpenTask(taskId) : undefined}
            >
              <span className="plan-recent-activity-task">{taskLabel}</span>
              <span className="plan-task-activity-kind">
                {color && <span className="plan-task-activity-kind-dot" style={{ background: color }} />}
                {kind}
              </span>
              <span className="plan-task-activity-age" title={milestoneAbsoluteTitle(milestone.ts)}>
                {milestoneAgeLabel(milestone.ts, now)}
              </span>
              {author && <span className="plan-recent-activity-author">{author}</span>}
              {snippet && <span className="plan-recent-activity-snippet">{snippet}</span>}
            </RowTag>
          );
        })}
      </div>
    </section>
  );
}
