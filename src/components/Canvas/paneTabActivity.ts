import type { Pane } from "../../lib/types";
import type { PlanIndexEntry } from "./plansApi";
import { findPlanIndexForPane, ownerIsPaneSelf } from "./planPaneMatch";

// EMERGENCE (north-star #2), OUTSIDE the canvas, on the TAB BAR: glance the whole
// swarm from the session list. A pane that OWNS a plan reads as an orchestrator; a
// pane DIRECTLY executing a plan's task reads as a worker; anything else gets nothing
// (honest — a tab merely nested under an orchestrator is not "working"). Index-ONLY
// by design: this runs for EVERY tab, so it stays the cheap gate (findPlanIndexForPane)
// and never pulls full plan bodies. The orchestrator/worker rule is the SAME one the
// above-composer strips use (ownerIsPaneSelf + direct assignment). Exact done/total
// and the task handle live on the full-body strip; the tab carries the glance.
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
