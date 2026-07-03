import type { Pane } from "../../lib/types";
import { resolvePaneCurrentTask } from "./paneCurrentTask";
import { resolvePaneOwnedPlans } from "./paneOwnedPlans";
import type { Plan, Task } from "./types";

function displayPlanTitle(plan: Plan): string {
  const title = plan.title?.trim();
  if (title) return title;
  return plan.id;
}

function displayTaskTitle(task: Task): string {
  const name = task.name?.trim();
  if (name) return name;
  const handle = task.handle?.trim();
  if (handle) return handle;
  return task.id;
}

export type PaneComposerActivity =
  | { kind: "orchestrating"; planId: string; planTitle: string; extraPlans: number }
  | { kind: "working"; planId: string; planTitle: string; taskId: string; taskTitle: string; taskStatus: import("./types").TaskStatus };

// Above-composer context: same ownership/assignment rules as paneTabActivity, but
// resolved from full plan bodies so we can show human task titles (never raw ids).
export function resolvePaneComposerActivity(
  plans: readonly Plan[],
  paneId: string | null | undefined,
  panes: Record<string, Pane>,
): PaneComposerActivity | null {
  if (!paneId) return null;
  const owned = resolvePaneOwnedPlans(plans, paneId, panes);
  if (owned.length > 0) {
    const lead = owned[0];
    return {
      kind: "orchestrating",
      planId: lead.plan.id,
      planTitle: displayPlanTitle(lead.plan),
      extraPlans: owned.length - 1,
    };
  }
  const current = resolvePaneCurrentTask(plans, paneId);
  if (!current) return null;
  return {
    kind: "working",
    planId: current.plan.id,
    planTitle: displayPlanTitle(current.plan),
    taskId: current.task.id,
    taskTitle: displayTaskTitle(current.task),
    taskStatus: current.task.status,
  };
}
