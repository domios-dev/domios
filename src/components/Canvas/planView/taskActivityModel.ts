import type { Tab } from "../../../lib/types";
import { formatTime } from "../../../lib/formatters";
import type { Milestone, Plan, Task } from "../types";
import { resolveTaskExecutionAttempts } from "../taskExecutorHistory";
import { formatRelativeAge } from "./planListModel";

export const VISIBLE_MILESTONE_COLLAPSE = 5;

export interface PlanActivityItem {
  taskId: string;
  taskLabel: string;
  milestone: Milestone;
}

export function milestoneKindLabel(kind: string): string {
  return kind.replace(/[_-]+/g, " ");
}

export function sortMilestonesNewestFirst(milestones: readonly Milestone[]): Milestone[] {
  return [...milestones].sort((a, b) => b.ts - a.ts);
}

export function resolveMilestoneAuthor(milestone: Milestone, tabs: Record<string, Tab>): string | null {
  const fromTitle = milestone.from.title?.trim();
  if (fromTitle) return fromTitle;
  const tabId = milestone.from.tab_id?.trim();
  if (!tabId) return null;
  return tabs[tabId]?.title?.trim() || null;
}

export function visibleMilestones(
  milestones: readonly Milestone[],
  expanded: boolean,
): { visible: Milestone[]; hiddenCount: number } {
  const sorted = sortMilestonesNewestFirst(milestones);
  if (expanded || sorted.length <= VISIBLE_MILESTONE_COLLAPSE) {
    return { visible: sorted, hiddenCount: 0 };
  }
  return {
    visible: sorted.slice(0, VISIBLE_MILESTONE_COLLAPSE),
    hiddenCount: sorted.length - VISIBLE_MILESTONE_COLLAPSE,
  };
}

export function buildPlanRecentActivity(plan: Plan, limit = 12): PlanActivityItem[] {
  const items: PlanActivityItem[] = [];
  for (const task of plan.tasks) {
    const taskLabel = task.handle?.trim() || task.name?.trim() || task.id;
    for (const milestone of task.milestones) {
      items.push({ taskId: task.id, taskLabel, milestone });
    }
  }
  items.sort((a, b) => b.milestone.ts - a.milestone.ts);
  return items.slice(0, limit);
}

export function taskHasActivity(task: Task, tabs: Record<string, Tab> = {}): boolean {
  return task.milestones.length > 0
    || resolveTaskExecutionAttempts(task, tabs).length > 0
    || (task.execution_attempts ?? []).some((attempt) => attempt.report?.diagnostics != null);
}

export function milestoneAgeLabel(ts: number, now: number): string {
  return formatRelativeAge(ts, now);
}

export function milestoneAbsoluteTitle(ts: number): string {
  return formatTime(ts);
}

// Visible absolute stamp: full "Jul 3, 18:42" normally, time-only for entries dated today.
export function milestoneTimestampLabel(ts: number, now: number): string {
  const date = new Date(ts);
  const ref = new Date(now);
  const sameDay =
    date.getFullYear() === ref.getFullYear() &&
    date.getMonth() === ref.getMonth() &&
    date.getDate() === ref.getDate();
  if (sameDay) return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return formatTime(ts);
}
