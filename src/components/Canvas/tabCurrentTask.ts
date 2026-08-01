import { TaskStatus, type Plan, type Task, type TaskStatus as TaskStatusType } from "./types";

export interface TabCurrentTask {
  plan: Plan;
  task: Task;
  branch: string | null;
}

export function isActiveTaskStatus(status: TaskStatusType): boolean {
  return status !== TaskStatus.Done && status !== TaskStatus.Cancelled && status !== TaskStatus.Paused;
}

// lane it physically runs in. We honor both (the index only carries the former).
export function taskBoundToTab(task: Task, tabId: string): boolean {
  if (!isActiveTaskStatus(task.status)) return false;
  return task.assigned_tab_id === tabId || task.lane?.tab_id === tabId;
}

const STATUS_RANK: Record<TaskStatusType, number> = {
  [TaskStatus.InProgress]: 0,
  [TaskStatus.InReview]: 1,
  [TaskStatus.Blocked]: 2,
  [TaskStatus.Pending]: 3,
  [TaskStatus.Paused]: 4,
  [TaskStatus.Done]: 5,
  [TaskStatus.Cancelled]: 6,
};

function freshnessOf(task: Task): number {
  return task.status_changed_at ?? task.updated_at ?? task.created_at ?? 0;
}

/** Negative when `a` is the more live task for the same tab. */
export function compareTaskLiveness(a: Task, b: Task): number {
  const rank = STATUS_RANK[a.status] - STATUS_RANK[b.status];
  return rank !== 0 ? rank : freshnessOf(b) - freshnessOf(a);
}

// assigned_tab_id / lane.tab_id rows; presence must follow the winner only.
export function resolvePlanExclusiveTabTasks(plan: Plan): Map<string, string> {
  const winners = new Map<string, Task>();
  for (const task of plan.tasks) {
    if (!isActiveTaskStatus(task.status)) continue;
    const tabIds = new Set<string>();
    const assigned = task.assigned_tab_id?.trim();
    const laneTab = task.lane?.tab_id?.trim();
    if (assigned) tabIds.add(assigned);
    if (laneTab) tabIds.add(laneTab);
    for (const tabId of tabIds) {
      const prev = winners.get(tabId);
      if (!prev || compareTaskLiveness(task, prev) < 0) winners.set(tabId, task);
    }
  }
  return new Map([...winners.entries()].map(([tabId, task]) => [tabId, task.id]));
}

export function resolveTabCurrentTask(
  plans: readonly Plan[],
  tabId: string | null | undefined,
): TabCurrentTask | null {
  if (!tabId) return null;
  const matches: TabCurrentTask[] = [];
  for (const plan of plans) {
    if (plan.status !== "active") continue;
    for (const task of plan.tasks) {
      if (taskBoundToTab(task, tabId)) {
        matches.push({ plan, task, branch: task.lane?.branch?.trim() || null });
      }
    }
  }
  if (matches.length === 0) return null;
  matches.sort((a, b) => compareTaskLiveness(a.task, b.task));
  return matches[0];
}
