import type { Tab } from "../../lib/types";
import { TaskStatus, type Plan } from "../Canvas/types";
import { PLAN_STATUS_COLORS } from "../shared/SegmentedDonut";
import type { PlanStatusCounts } from "../shared/PlanStatusBreakdown";
export interface TeamTaskBucket {
  key: "todo" | "review" | "blocked" | "paused" | "done";
  label: string;
  count: number;
  color: string;
}

export const EMPTY_BUCKETS: TeamTaskBucket[] = [
  { key: "todo", label: "todo", count: 0, color: PLAN_STATUS_COLORS.todo },
  { key: "review", label: "review", count: 0, color: PLAN_STATUS_COLORS.review },
  { key: "blocked", label: "blocked", count: 0, color: PLAN_STATUS_COLORS.blocked },
  { key: "paused", label: "paused", count: 0, color: PLAN_STATUS_COLORS.paused },
  { key: "done", label: "done", count: 0, color: PLAN_STATUS_COLORS.done },
];

export function deriveTeamTaskBuckets(
  plan: Plan,
  _tabs: Record<string, Tab>,
): TeamTaskBucket[] {
  const counts = new Map<TeamTaskBucket["key"], number>(EMPTY_BUCKETS.map((bucket) => [bucket.key, 0]));
  for (const task of plan.tasks) {
    const key: TeamTaskBucket["key"] | null =
      task.status === TaskStatus.Pending || task.status === TaskStatus.InProgress
        ? "todo"
        : task.status === TaskStatus.InReview
          ? "review"
          : task.status === TaskStatus.Blocked
            ? "blocked"
            : task.status === TaskStatus.Paused
              ? "paused"
              : task.status === TaskStatus.Done
                ? "done"
                : null;
    if (key) counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return EMPTY_BUCKETS.map((bucket) => ({
    ...bucket,
    count: counts.get(bucket.key) ?? 0,
  }));
}

export function deriveCardStatusCounts(buckets: TeamTaskBucket[]): PlanStatusCounts {
  const count = (key: TeamTaskBucket["key"]) => buckets.find((bucket) => bucket.key === key)?.count ?? 0;
  const paused = count("paused");
  return {
    todo: count("todo") + paused,
    review: count("review"),
    blocked: count("blocked"),
    paused,
    done: count("done"),
  };
}

