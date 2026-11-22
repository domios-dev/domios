import { tabActivity } from "../../lib/tabActivity";
import type { Tab, TabStatus } from "../../lib/types";

function effectiveSidebarStatus(tab: Tab | undefined, status: TabStatus | null | undefined): TabStatus | null {
  const activity = tabActivity(tab, status);
  if (activity.kind === "idle" && activity.backgroundTasks > 0) return "Working";
  return activity.status;
}

export function aggregateTabStatuses(
  workspaceTabIds: string[],
  statuses: Record<string, TabStatus>,
  tabs: Record<string, Tab>,
): TabStatus | null {
  const priority: TabStatus[] = ["Dead", "Error", "Waiting", "Working", "Idle"];
  let worst: TabStatus | null = null;
  for (const pid of workspaceTabIds) {
    const s = effectiveSidebarStatus(tabs[pid], statuses[pid]);
    if (!s) continue;
    if (!worst || priority.indexOf(s) < priority.indexOf(worst)) {
      worst = s;
    }
  }
  return worst;
}

export function countWorkingTabs(
  tabs: readonly Tab[],
  statuses: Record<string, TabStatus | null | undefined>,
): number {
  return tabs.filter((tab) => {
    const activity = tabActivity(tab, statuses[tab.id]);
    return activity.active || (activity.kind === "idle" && activity.backgroundTasks > 0);
  }).length;
}
