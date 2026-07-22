import type { Pane } from "../../lib/types";

export interface OrchestrationGroupMember {
  paneId: string;
  title: string;
  role: string | null;
  /** Manager specialization, for badges and summaries. */
  roleProfile: string | null;
}

/** All descendants of an Orchestrator pane (Planner + Workers), depth-first —
 *  what actually disappears if the group is closed as a unit (task_e0ddee08 item 2). */
export function collectOrchestrationGroupMembers(
  orchestratorPaneId: string,
  panes: Record<string, Pane>,
): OrchestrationGroupMember[] {
  const members: OrchestrationGroupMember[] = [];
  const visit = (parentId: string) => {
    for (const pane of Object.values(panes)) {
      if (pane.parent_pane_id === parentId) {
        members.push({
          paneId: pane.id,
          title: pane.title,
          role: pane.role ?? null,
          roleProfile: pane.role_profile ?? null,
        });
        visit(pane.id);
      }
    }
  };
  visit(orchestratorPaneId);
  return members;
}
