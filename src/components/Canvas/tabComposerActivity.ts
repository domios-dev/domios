import type { Tab } from "../../lib/types";
import { resolvePlannerTabIdFromPlan } from "../shared/resolvePlanPeerPair";
import { resolveTabCurrentTask } from "./tabCurrentTask";
import { resolveTabOwnedPlans } from "./tabOwnedPlans";
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

export type TabComposerActivity =
  | { kind: "orchestrating"; planId: string; planTitle: string; extraPlans: number }
  | { kind: "planning"; planId: string; planTitle: string }
  | { kind: "working"; planId: string; planTitle: string; taskId: string; taskTitle: string; taskStatus: import("./types").TaskStatus };

// resolved from full plan bodies so we can show human task titles (never raw ids).
export function resolveTabComposerActivity(
  plans: readonly Plan[],
  tabId: string | null | undefined,
  tabs: Record<string, Tab>,
): TabComposerActivity | null {
  if (!tabId) return null;
  const owned = resolveTabOwnedPlans(plans, tabId, tabs);
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
    (p) => p.status === "active" && resolvePlannerTabIdFromPlan(p) === tabId,
  );
  if (planned) {
    return { kind: "planning", planId: planned.id, planTitle: displayPlanTitle(planned) };
  }
  const current = resolveTabCurrentTask(plans, tabId);
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
