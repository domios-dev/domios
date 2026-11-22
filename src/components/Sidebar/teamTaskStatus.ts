import type { PlanView as Plan } from "../Canvas/planProjection";
import type { Tab } from "../../lib/types";
import { TaskStatus } from "../Canvas/types";
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
  if (plan.task_counts) {
    const counts = plan.task_counts;
    const values = { todo: (counts.pending ?? 0) + (counts.in_progress ?? 0), review: counts.in_review ?? 0,
      blocked: counts.blocked ?? 0, paused: counts.paused ?? 0, done: counts.done ?? 0 };
    return EMPTY_BUCKETS.map((bucket) => ({ ...bucket, count: values[bucket.key] }));
  }
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

export interface TeamPlanCountMeta {
  indexOnly?: boolean;
  indexTaskCount?: number | null;
  indexOpenTaskCount?: number | null;
  indexUpdatedAt?: number | null;
  loading?: boolean;
  unavailable?: boolean;
  stale?: boolean;
}

export type TeamPlanStatusSummary =
  | { state: "verified"; buckets: TeamTaskBucket[]; counts: PlanStatusCounts; stale: boolean }
  | { state: "unavailable"; totalTasks: number | null; openTasks: number | null; loading: boolean }
  | { state: "absent"; buckets: TeamTaskBucket[]; counts: PlanStatusCounts };

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

export interface RemoteTeamPlanMatch {
  plan: Plan | null;
  meta: TeamPlanCountMeta;
}

export function selectRemoteTeamPlanProjection(input: {
  teamId: string | null | undefined;
  owningPeerId: string;
  viewedPeerId: string;
  plans: readonly (Plan & { team_id?: string | null })[];
  available: boolean;
  loading: boolean;
  reconnecting: boolean;
  error: string | null;
}): RemoteTeamPlanMatch {
  if (!input.teamId || input.owningPeerId !== input.viewedPeerId) {
    return { plan: null, meta: { unavailable: true, loading: false } };
  }
  let matched: Plan | null = null;
  for (const candidate of input.plans) {
    if (candidate.team_id !== input.teamId || candidate.status !== "active") continue;
    const currentUpdatedAt = matched?.updated_at ?? matched?.created_at ?? 0;
    const updatedAt = candidate.updated_at ?? candidate.created_at ?? 0;
    if (!matched || updatedAt > currentUpdatedAt || (updatedAt === currentUpdatedAt && candidate.id > matched.id)) {
      matched = candidate;
    }
  }
  if (matched) {
    return {
      plan: matched,
      meta: {
        loading: input.loading,
        unavailable: Boolean(input.error),
        stale: input.reconnecting,
      },
    };
  }
  if (!input.available || input.loading || Boolean(input.error)) {
    return {
      plan: null,
      meta: {
        loading: input.loading,
        unavailable: true,
      },
    };
  }
  return {
    plan: null,
    meta: {
      loading: false,
      unavailable: false,
    },
  };
}

export function resolveTeamPlanStatus(
  plan: Plan | null,
  tabs: Record<string, Tab>,
  meta?: TeamPlanCountMeta,
): TeamPlanStatusSummary {
  if (!plan) {
    if (meta?.loading || meta?.unavailable) {
      return {
        state: "unavailable",
        totalTasks: null,
        openTasks: null,
        loading: Boolean(meta.loading),
      };
    }
    return {
      state: "absent",
      buckets: EMPTY_BUCKETS,
      counts: deriveCardStatusCounts(EMPTY_BUCKETS),
    };
  }
  const summaryUpdatedAt = plan.updated_at ?? plan.created_at ?? 0;
  const indexUpdatedAt = meta?.indexUpdatedAt ?? null;
  const indexNewer = indexUpdatedAt !== null && indexUpdatedAt > summaryUpdatedAt;
  const indexAtLeastAsNew = indexUpdatedAt === null || indexUpdatedAt >= summaryUpdatedAt;
  if (plan.task_counts) {
    const summaryTotal = plan.task_counts.total;
    if (
      summaryTotal === 0
      && ((indexAtLeastAsNew && (meta?.indexTaskCount ?? 0) > 0)
        || (indexNewer && meta?.indexTaskCount == null))
    ) {
      return {
        state: "unavailable",
        totalTasks: meta?.indexTaskCount ?? null,
        openTasks: meta?.indexOpenTaskCount ?? null,
        loading: Boolean(meta?.loading || !meta?.unavailable),
      };
    }
    const buckets = deriveTeamTaskBuckets(plan, tabs);
    return {
      state: "verified",
      buckets,
      counts: deriveCardStatusCounts(buckets),
      stale: Boolean(meta?.stale || indexNewer),
    };
  }
  const hasTruncatedWindow = "task_window" in plan && Boolean((plan as { task_window?: unknown }).task_window);
  if (hasTruncatedWindow) {
    return {
      state: "unavailable",
      totalTasks: meta?.indexTaskCount ?? null,
      openTasks: meta?.indexOpenTaskCount ?? null,
      loading: Boolean(meta?.loading),
    };
  }
  if (meta?.indexOnly) {
    if (meta.indexTaskCount === 0) {
      return {
        state: "verified",
        buckets: EMPTY_BUCKETS,
        counts: deriveCardStatusCounts(EMPTY_BUCKETS),
        stale: Boolean(meta.stale),
      };
    }
    return {
      state: "unavailable",
      totalTasks: meta.indexTaskCount ?? null,
      openTasks: meta.indexOpenTaskCount ?? null,
      loading: Boolean(meta.loading),
    };
  }
  const summaryTotal = plan.tasks.length;
  if (
    summaryTotal === 0
    && ((indexAtLeastAsNew && (meta?.indexTaskCount ?? 0) > 0)
      || (indexNewer && meta?.indexTaskCount == null))
  ) {
    return {
      state: "unavailable",
      totalTasks: meta?.indexTaskCount ?? null,
      openTasks: meta?.indexOpenTaskCount ?? null,
      loading: Boolean(meta?.loading || !meta?.unavailable),
    };
  }
  const buckets = deriveTeamTaskBuckets(plan, tabs);
  return {
    state: "verified",
    buckets,
    counts: deriveCardStatusCounts(buckets),
    stale: Boolean(meta?.stale || indexNewer),
  };
}

