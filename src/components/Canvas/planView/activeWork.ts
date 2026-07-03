import type { Pane } from "../../../lib/types";
import { buildTaskPresence } from "../taskPresence";
import { resolvePlanExclusivePaneTasks, taskBoundToPane } from "../paneCurrentTask";
import { formatRelativeAge } from "./planListModel";
import type { PlanSwitcherOption } from "./PlanSwitcher";
import { TaskStatus, type Plan, type Task, type TaskStatus as TaskStatusType } from "../types";

// Mirror src-tauri/src/plan/reconcile.rs — keep thresholds aligned with plan-state drift.
export const IN_REVIEW_UNJUDGED_STALE_MS = 2 * 60 * 60 * 1000;
export const IN_PROGRESS_ACTIVITY_STALE_MS = 4 * 60 * 60 * 1000;

const IN_FLIGHT = new Set<TaskStatusType>([TaskStatus.InProgress, TaskStatus.InReview]);

export interface ActiveWorkEntry {
  planId: string;
  planTitle: string;
  task: Task;
  status: TaskStatusType;
  executorPaneId: string | null;
  executorLabel: string | null;
  executorPresent: boolean;
  freshnessTs: number;
  freshnessLabel: string;
  stale: boolean;
}

export interface ActiveWorkSnapshot {
  entries: ActiveWorkEntry[];
  byPlanId: ReadonlyMap<string, readonly ActiveWorkEntry[]>;
  countByPlanId: ReadonlyMap<string, number>;
}

function displayPlanTitle(plan: Plan): string {
  const title = plan.title?.trim();
  return title || plan.id;
}

export function displayTaskTitle(task: Task): string {
  const name = task.name?.trim();
  if (name) return name;
  const handle = task.handle?.trim();
  if (handle) return handle;
  return task.id;
}

function executorPaneId(task: Task): string | null {
  return task.assigned_pane_id?.trim() || task.lane?.pane_id?.trim() || null;
}

function executorLabel(paneId: string | null, panes: Record<string, Pane>): string | null {
  if (!paneId) return null;
  const title = panes[paneId]?.title?.trim();
  if (title) return title;
  return paneId.length > 12 ? paneId.slice(0, 8) : paneId;
}

export function taskActivityFreshnessTs(task: Task): number {
  let max = Math.max(task.status_changed_at ?? 0, task.updated_at ?? 0, task.created_at ?? 0);
  for (const m of task.milestones) max = Math.max(max, m.ts ?? 0);
  return max;
}

export function isActiveWorkStatus(status: TaskStatusType): boolean {
  return IN_FLIGHT.has(status);
}

export function isActiveWorkStale(task: Task, now: number): boolean {
  if (task.status === TaskStatus.InReview) {
    const changed = task.status_changed_at ?? 0;
    return changed > 0 && now - changed > IN_REVIEW_UNJUDGED_STALE_MS;
  }
  if (task.status === TaskStatus.InProgress) {
    const fresh = taskActivityFreshnessTs(task);
    return fresh > 0 && now - fresh > IN_PROGRESS_ACTIVITY_STALE_MS;
  }
  return false;
}

function buildEntry(plan: Plan, task: Task, panes: Record<string, Pane>, now: number): ActiveWorkEntry {
  const paneId = executorPaneId(task);
  const present = paneId != null && paneId in panes;
  const freshTs = taskActivityFreshnessTs(task);
  return {
    planId: plan.id,
    planTitle: displayPlanTitle(plan),
    task,
    status: task.status,
    executorPaneId: paneId,
    executorLabel: executorLabel(paneId, panes),
    executorPresent: present,
    freshnessTs: freshTs,
    freshnessLabel: formatRelativeAge(freshTs, now),
    stale: isActiveWorkStale(task, now),
  };
}

function compareEntries(a: ActiveWorkEntry, b: ActiveWorkEntry): number {
  if (a.stale !== b.stale) return a.stale ? 1 : -1;
  const statusRank = (s: TaskStatusType) => (s === TaskStatus.InProgress ? 0 : 1);
  const sr = statusRank(a.status) - statusRank(b.status);
  if (sr !== 0) return sr;
  const fresh = b.freshnessTs - a.freshnessTs;
  if (fresh !== 0) return fresh;
  const ta = displayTaskTitle(a.task).toLowerCase();
  const tb = displayTaskTitle(b.task).toLowerCase();
  if (ta !== tb) return ta < tb ? -1 : 1;
  return a.task.id < b.task.id ? -1 : a.task.id > b.task.id ? 1 : 0;
}

export function selectActiveWork(
  plans: readonly Plan[],
  panes: Record<string, Pane>,
  now: number = Date.now(),
): ActiveWorkSnapshot {
  const entries: ActiveWorkEntry[] = [];
  for (const plan of plans) {
    if (plan.status !== "active") continue;
    for (const task of plan.tasks) {
      if (!isActiveWorkStatus(task.status)) continue;
      entries.push(buildEntry(plan, task, panes, now));
    }
  }
  entries.sort(compareEntries);

  const byPlanId = new Map<string, ActiveWorkEntry[]>();
  const countByPlanId = new Map<string, number>();
  for (const entry of entries) {
    const bucket = byPlanId.get(entry.planId) ?? [];
    bucket.push(entry);
    byPlanId.set(entry.planId, bucket);
    countByPlanId.set(entry.planId, bucket.length);
  }
  return { entries, byPlanId, countByPlanId };
}

export function activeWorkForPane(
  snapshot: ActiveWorkSnapshot,
  paneId: string | null | undefined,
): ActiveWorkEntry | null {
  if (!paneId) return null;
  for (const entry of snapshot.entries) {
    if (taskBoundToPane(entry.task, paneId)) return entry;
  }
  return null;
}

export function activeWorkForPlan(
  snapshot: ActiveWorkSnapshot,
  planId: string,
): readonly ActiveWorkEntry[] {
  return snapshot.byPlanId.get(planId) ?? [];
}

export function inflightTooltip(entries: readonly ActiveWorkEntry[]): string {
  if (entries.length === 0) return "";
  return entries
    .map((e) => {
      const who = e.executorLabel ?? "unassigned";
      const stale = e.stale ? " · stale" : "";
      return `${displayTaskTitle(e.task)} (${e.status.replace(/_/g, " ")}, ${who}${stale})`;
    })
    .join("\n");
}

export function activeWorkPresenceLive(
  plan: Plan,
  task: Task,
  panes: Record<string, Pane>,
  exclusivePaneTasks: ReadonlyMap<string, string>,
): boolean {
  const presence = buildTaskPresence(plan, task, panes, exclusivePaneTasks);
  return presence != null && task.status === TaskStatus.InProgress;
}

export function enrichSwitcherOptions(
  options: readonly PlanSwitcherOption[],
  snapshot: ActiveWorkSnapshot,
): PlanSwitcherOption[] {
  return options.map((opt) => {
    const entries = activeWorkForPlan(snapshot, opt.id);
    if (entries.length === 0) return opt;
    return {
      ...opt,
      inflightCount: entries.length,
      inflightTooltip: inflightTooltip(entries),
    };
  });
}

export function buildExclusivePaneTasksByPlan(plans: readonly Plan[]): Map<string, Map<string, string>> {
  const out = new Map<string, Map<string, string>>();
  for (const plan of plans) out.set(plan.id, resolvePlanExclusivePaneTasks(plan));
  return out;
}
