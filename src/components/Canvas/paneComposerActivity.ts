import type { Pane } from "../../lib/types";
import { resolvePlannerPaneIdFromPlan } from "../shared/resolvePlanPeerPair";
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
  | { kind: "planning"; planId: string; planTitle: string }
  | { kind: "working"; planId: string; planTitle: string; taskId: string; taskTitle: string; taskStatus: import("./types").TaskStatus };

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
  // "Planning:", the manager-tier mirror of the orchestrator strip.
  const planned = plans.find(
    (p) => p.status === "active" && resolvePlannerPaneIdFromPlan(p) === paneId,
  );
  if (planned) {
    return { kind: "planning", planId: planned.id, planTitle: displayPlanTitle(planned) };
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
