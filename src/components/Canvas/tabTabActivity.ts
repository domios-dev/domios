import type { Tab } from "../../lib/types";
import type { PlanIndexEntry } from "./plansApi";
import { findPlanIndexForTab, ownerIsTabSelf } from "./planTabMatch";

export interface TabTabActivity {
  kind: "orchestrator" | "worker";
  planId: string;
  planTitle: string;
  /** Tasks in the lead plan (index count) — orchestrator scale at a glance. */
  taskCount: number;
  /** How many active plans this tab relates to — drives a "+N" hover hint. */
  planCount: number;
}

// Most-recently-updated wins — the plan most likely in flight for this tab.
function leadOf(entries: PlanIndexEntry[]): PlanIndexEntry {
  return entries.reduce((a, b) => (b.updated_at > a.updated_at ? b : a));
}

export function resolveTabTabActivity(
  index: PlanIndexEntry[],
  tabId: string | null | undefined,
  tabs: Record<string, Tab>,
): TabTabActivity | null {
  if (!tabId) return null;
  const entries = findPlanIndexForTab(index, tabId, tabs);
  if (entries.length === 0) return null;

  const sessionId = tabs[tabId]?.agent_session_id?.trim() || null;
  const owned = entries.filter((e) => ownerIsTabSelf(e.owner, tabId, sessionId));
  if (owned.length > 0) {
    const lead = leadOf(owned);
    return { kind: "orchestrator", planId: lead.plan_id, planTitle: lead.title, taskCount: lead.task_count ?? 0, planCount: owned.length };
  }

  const assigned = entries.filter((e) => e.assigned_tab_ids.includes(tabId));
  if (assigned.length > 0) {
    const lead = leadOf(assigned);
    return { kind: "worker", planId: lead.plan_id, planTitle: lead.title, taskCount: lead.task_count ?? 0, planCount: assigned.length };
  }

  return null;
}
