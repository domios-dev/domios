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
import { ActivityLogRow } from "./ActivityLogRow";

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
    <section className="ct-activity plan-recent-activity" data-testid="plan-recent-activity" aria-label="Recent activity">
      <h3 className="ct-activity__title">Recent activity</h3>
      <div className="plan-recent-activity-rail">
        {items.map(({ taskId, taskLabel, milestone }) => (
          <ActivityLogRow
            key={`${taskId}:${milestone.id}`}
            kind={milestoneKindLabel(milestone.kind)}
            color={colors.get(milestone.kind)}
            age={milestoneAgeLabel(milestone.ts, now)}
            stampTitle={milestoneAbsoluteTitle(milestone.ts)}
            author={resolveMilestoneAuthor(milestone, tabs)}
            lead={taskLabel}
            text={milestone.text}
            onOpen={onOpenTask ? () => onOpenTask(taskId) : undefined}
            testIds={{ row: "plan-recent-activity-item" }}
            data={{ "task-id": taskId, "milestone-id": milestone.id }}
          />
        ))}
      </div>
    </section>
  );
}
