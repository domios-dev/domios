import type { Tab } from "../../lib/types";

export interface OrchestrationGroupMember {
  tabId: string;
  title: string;
  role: string | null;
  /** Manager specialization, for badges and summaries. */
  roleProfile: string | null;
}

/** All descendants of an Orchestrator tab (Planner + Workers), depth-first —
 *  what actually disappears if the group is closed as a unit (task_e0ddee08 item 2). */
export function collectOrchestrationGroupMembers(
  orchestratorTabId: string,
  tabs: Record<string, Tab>,
): OrchestrationGroupMember[] {
  const members: OrchestrationGroupMember[] = [];
  const visit = (parentId: string) => {
    for (const tab of Object.values(tabs)) {
      if (tab.parent_tab_id === parentId) {
        members.push({
          tabId: tab.id,
          title: tab.title,
          role: tab.role ?? null,
          roleProfile: tab.role_profile ?? null,
        });
        visit(tab.id);
      }
    }
  };
  visit(orchestratorTabId);
  return members;
}
