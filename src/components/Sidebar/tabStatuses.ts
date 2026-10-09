import { tabActivity } from "../../lib/tabActivity";
import type { Tab, TabStatus } from "../../lib/types";

export function aggregateTabStatuses(
  workspaceTabIds: string[],
  statuses: Record<string, TabStatus>,
  tabs: Record<string, Tab>,
): TabStatus | null {
  const priority: TabStatus[] = ["Dead", "Error", "Waiting", "Working", "Idle"];
  let worst: TabStatus | null = null;
  for (const pid of workspaceTabIds) {
    const s = tabActivity(tabs[pid], statuses[pid]).status;
    if (!s) continue;
    if (!worst || priority.indexOf(s) < priority.indexOf(worst)) {
      worst = s;
    }
  }
  return worst;
}
