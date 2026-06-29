import { TaskStatus, type Plan, type Task, type TaskStatus as TaskStatusType } from "./types";

export interface PaneCurrentTask {
  plan: Plan;
  task: Task;
  branch: string | null;
}

// A worker pane owns its task via either binding: the explicit assignment or the
// lane it physically runs in. We honor both (the index only carries the former).
export function taskBoundToPane(task: Task, paneId: string): boolean {
  return task.assigned_pane_id === paneId || task.lane?.pane_id === paneId;
}

// "Current" = the most live of this pane's tasks. A pane normally executes one,
// but a re-bound lane can leave stale done rows — rank by liveness so the strip
// shows what's actually in flight, freshest first on a tie.
const STATUS_RANK: Record<TaskStatusType, number> = {
  [TaskStatus.InProgress]: 0,
  [TaskStatus.InReview]: 1,
  [TaskStatus.Blocked]: 2,
  [TaskStatus.Pending]: 3,
  [TaskStatus.Done]: 4,
  [TaskStatus.Cancelled]: 5,
};

function freshnessOf(task: Task): number {
  return task.status_changed_at ?? task.updated_at ?? task.created_at ?? 0;
}

/** Negative when `a` is the more live task for the same pane. */
export function compareTaskLiveness(a: Task, b: Task): number {
  const rank = STATUS_RANK[a.status] - STATUS_RANK[b.status];
  return rank !== 0 ? rank : freshnessOf(b) - freshnessOf(a);
}

// One live pane → one current task per plan. Reassignment can leave stale
// assigned_pane_id / lane.pane_id rows; presence must follow the winner only.
export function resolvePlanExclusivePaneTasks(plan: Plan): Map<string, string> {
  const winners = new Map<string, Task>();
  for (const task of plan.tasks) {
    const paneIds = new Set<string>();
    const assigned = task.assigned_pane_id?.trim();
    const lanePane = task.lane?.pane_id?.trim();
    if (assigned) paneIds.add(assigned);
    if (lanePane) paneIds.add(lanePane);
    for (const paneId of paneIds) {
      const prev = winners.get(paneId);
      if (!prev || compareTaskLiveness(task, prev) < 0) winners.set(paneId, task);
    }
  }
  return new Map([...winners.entries()].map(([paneId, task]) => [paneId, task.id]));
}

export function resolvePaneCurrentTask(
  plans: readonly Plan[],
  paneId: string | null | undefined,
): PaneCurrentTask | null {
  if (!paneId) return null;
  const matches: PaneCurrentTask[] = [];
  for (const plan of plans) {
    if (plan.status !== "active") continue;
    for (const task of plan.tasks) {
      if (taskBoundToPane(task, paneId)) {
        matches.push({ plan, task, branch: task.lane?.branch?.trim() || null });
      }
    }
  }
  if (matches.length === 0) return null;
  matches.sort((a, b) => compareTaskLiveness(a.task, b.task));
  return matches[0];
}
