import type { Pane } from "../../lib/types";
import type { PlanIndexEntry } from "./plansApi";
import { findPlanIndexForPane, ownerIsPaneSelf } from "./planPaneMatch";

export interface PaneTabActivity {
  kind: "orchestrator" | "worker";
  planId: string;
  planTitle: string;
  /** Tasks in the lead plan (index count) — orchestrator scale at a glance. */
  taskCount: number;
  /** How many active plans this pane relates to — drives a "+N" hover hint. */
  planCount: number;
}

// Most-recently-updated wins — the plan most likely in flight for this pane.
function leadOf(entries: PlanIndexEntry[]): PlanIndexEntry {
  return entries.reduce((a, b) => (b.updated_at > a.updated_at ? b : a));
}

export function resolvePaneTabActivity(
  index: PlanIndexEntry[],
  paneId: string | null | undefined,
  panes: Record<string, Pane>,
): PaneTabActivity | null {
  if (!paneId) return null;
  const entries = findPlanIndexForPane(index, paneId, panes);
  if (entries.length === 0) return null;

  const sessionId = panes[paneId]?.agent_session_id?.trim() || null;
  const owned = entries.filter((e) => ownerIsPaneSelf(e.owner, paneId, sessionId));
  if (owned.length > 0) {
    const lead = leadOf(owned);
    return { kind: "orchestrator", planId: lead.plan_id, planTitle: lead.title, taskCount: lead.task_count ?? 0, planCount: owned.length };
  }

  const assigned = entries.filter((e) => e.assigned_pane_ids.includes(paneId));
  if (assigned.length > 0) {
    const lead = leadOf(assigned);
    return { kind: "worker", planId: lead.plan_id, planTitle: lead.title, taskCount: lead.task_count ?? 0, planCount: assigned.length };
  }

  return null;
}
