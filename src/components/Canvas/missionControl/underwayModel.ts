import type { Tab } from "../../../lib/types";
import { roleKind } from "../../../lib/roleKind";
import type { Plan } from "../types";
import { showsChecklist, type AgentChecklistItem } from "../../Sidebar/agentChecklistModel";

function rank(tab: Tab): number {
  const kind = roleKind(tab);
  return kind === "release_manager" ? 0 : kind === "qa" ? 1 : kind && kind !== "worker" ? 2 : 3;
}

export function underwayRows(tabs: readonly Tab[], plans: readonly Plan[], teamId: string) {
  return tabs.filter((tab) => tab.team_id === teamId && showsChecklist(tab)).map((tab) => {
    const assigned = plans.filter((plan) => plan.team_id === teamId).flatMap((plan) => plan.tasks).filter((candidate) => candidate.assigned_tab_id === tab.id);
    const task = assigned.find((candidate) => candidate.status === "in_progress") ?? assigned[0];
    return { tab, task };
  }).sort((a, b) => rank(a.tab) - rank(b.tab)
    || (a.task?.key ?? "").localeCompare(b.task?.key ?? "", undefined, { numeric: true })
    || a.tab.id.localeCompare(b.tab.id));
}

export function checklistProgress(items: readonly AgentChecklistItem[]) {
  return { done: items.filter((item) => item.state === "done").length, total: items.filter((item) => item.state !== "skipped").length };
}
