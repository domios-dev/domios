import type { Plan } from "./types";

export interface CanvasFocusPayload {
  task_id?: string | null;
  plan_id?: string | null;
  pane_id?: string | null;
  node_ids?: string[];
  plan_ids?: string[];
  scale?: number;
}

export function resolveCanvasFocusNodeIds(
  payload: CanvasFocusPayload | null | undefined,
  plans: readonly Plan[],
): string[] {
  const p = payload ?? {};
  const ids: string[] = [...(p.node_ids ?? []), ...(p.plan_ids ?? [])];
  if (p.task_id) ids.push(p.task_id);
  if (p.plan_id) ids.push(p.plan_id);
  if (p.pane_id) {
    for (const plan of plans)
      for (const t of plan.tasks) if (t.assigned_pane_id === p.pane_id) ids.push(t.id);
  }
  return ids;
}
