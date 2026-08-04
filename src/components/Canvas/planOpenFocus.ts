import type { Plan } from "./types";
import { isActiveTaskStatus } from "./tabCurrentTask";

export function assignedTaskIdsForTab(plans: readonly Plan[], tabId: string): string[] {
  const targetTabId = tabId.trim();
  if (!targetTabId) return [];
  return plans.flatMap((p) =>
    p.tasks
      .filter((t) => isActiveTaskStatus(t.status))
      .filter((t) => t.assigned_tab_id?.trim() === targetTabId)
      .map((t) => t.id),
  );
}

export type PlanOpenCameraTarget =
  | { kind: "task"; nodeIds: string[] }
  | { kind: "plan"; nodeIds: string[] }
  | { kind: "bounds" };

export function resolvePlanOpenCameraTarget(
  plans: readonly Plan[],
  focusTabId: string | null | undefined,
  activePlanId: string | null,
): PlanOpenCameraTarget {
  if (focusTabId) {
    const taskIds = assignedTaskIdsForTab(plans, focusTabId);
    if (taskIds.length) return { kind: "task", nodeIds: taskIds };
  }
  if (activePlanId) return { kind: "plan", nodeIds: [activePlanId] };
  return { kind: "bounds" };
}
