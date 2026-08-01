import type { Tab } from "../../../lib/types";
import { buildTaskPresence } from "../taskPresence";
import { resolvePlanExclusiveTabTasks, taskBoundToTab } from "../tabCurrentTask";
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
  executorTabId: string | null;
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

function executorTabId(task: Task): string | null {
  return task.assigned_tab_id?.trim() || task.lane?.tab_id?.trim() || null;
}

function executorLabel(tabId: string | null, tabs: Record<string, Tab>): string | null {
  if (!tabId) return null;
  const title = tabs[tabId]?.title?.trim();
  if (title) return title;
  return tabId.length > 12 ? tabId.slice(0, 8) : tabId;
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

function buildEntry(plan: Plan, task: Task, tabs: Record<string, Tab>, now: number): ActiveWorkEntry {
  const tabId = executorTabId(task);
  const present = tabId != null && tabId in tabs;
  const freshTs = taskActivityFreshnessTs(task);
  return {
    planId: plan.id,
    planTitle: displayPlanTitle(plan),
    task,
    status: task.status,
    executorTabId: tabId,
    executorLabel: executorLabel(tabId, tabs),
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
  tabs: Record<string, Tab>,
  now: number = Date.now(),
): ActiveWorkSnapshot {
  const entries: ActiveWorkEntry[] = [];
  for (const plan of plans) {
    if (plan.status !== "active") continue;
    if (plan.mode === "paused") continue;
    for (const task of plan.tasks) {
      if (!isActiveWorkStatus(task.status)) continue;
      entries.push(buildEntry(plan, task, tabs, now));
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

export function activeWorkForTab(
  snapshot: ActiveWorkSnapshot,
  tabId: string | null | undefined,
): ActiveWorkEntry | null {
  if (!tabId) return null;
  for (const entry of snapshot.entries) {
    if (taskBoundToTab(entry.task, tabId)) return entry;
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
  tabs: Record<string, Tab>,
  exclusiveTabTasks: ReadonlyMap<string, string>,
): boolean {
  const presence = buildTaskPresence(plan, task, tabs, exclusiveTabTasks);
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

export function buildExclusiveTabTasksByPlan(plans: readonly Plan[]): Map<string, Map<string, string>> {
  const out = new Map<string, Map<string, string>>();
  for (const plan of plans) out.set(plan.id, resolvePlanExclusiveTabTasks(plan));
  return out;
}
