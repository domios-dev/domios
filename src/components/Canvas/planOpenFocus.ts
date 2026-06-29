import type { Plan } from "./types";

export function assignedTaskIdsForPane(plans: readonly Plan[], paneId: string): string[] {
  return plans.flatMap((p) =>
    p.tasks.filter((t) => t.assigned_pane_id === paneId).map((t) => t.id),
  );
}

export type PlanOpenCameraTarget =
  | { kind: "task"; nodeIds: string[] }
  | { kind: "plan"; nodeIds: string[] }
  | { kind: "bounds" };

export function resolvePlanOpenCameraTarget(
  plans: readonly Plan[],
  focusPaneId: string | null | undefined,
  activePlanId: string | null,
): PlanOpenCameraTarget {
  if (focusPaneId) {
    const taskIds = assignedTaskIdsForPane(plans, focusPaneId);
    if (taskIds.length) return { kind: "task", nodeIds: taskIds };
  }
  if (activePlanId) return { kind: "plan", nodeIds: [activePlanId] };
  return { kind: "bounds" };
}
