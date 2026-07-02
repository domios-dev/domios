import type { Pane } from "../../lib/types";
import { resolvePaneCurrentTask } from "./paneCurrentTask";
import { resolvePaneOwnedPlans } from "./paneOwnedPlans";
import type { Plan } from "./types";

export type PaneComposerActivity =
  | { kind: "orchestrating"; planId: string; planTitle: string; extraPlans: number }
  | { kind: "working"; planId: string; planTitle: string; taskId: string; taskTitle: string };

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
      planTitle: lead.plan.title,
      extraPlans: owned.length - 1,
    };
  }
  const current = resolvePaneCurrentTask(plans, paneId);
  if (!current) return null;
  return {
    kind: "working",
    planId: current.plan.id,
    planTitle: current.plan.title,
    taskId: current.task.id,
    taskTitle: current.task.name,
  };
}
