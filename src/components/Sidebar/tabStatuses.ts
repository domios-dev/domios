import type { TabStatus } from "../../lib/types";

export function aggregateTabStatuses(
  workspaceTabIds: string[],
  statuses: Record<string, TabStatus>,
): TabStatus | null {
  const priority: TabStatus[] = ["Dead", "Error", "Waiting", "Working", "Idle"];
  let worst: TabStatus | null = null;
  for (const pid of workspaceTabIds) {
    const s = statuses[pid];
    if (!s) continue;
    if (!worst || priority.indexOf(s) < priority.indexOf(worst)) {
      worst = s;
    }
  }
  return worst;
}
