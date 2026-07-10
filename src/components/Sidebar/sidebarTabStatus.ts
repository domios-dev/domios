import type { PaneStatus } from "../../lib/types";

export function aggregateTabStatus(
  paneIds: string[],
  statuses: Record<string, PaneStatus>,
): PaneStatus | null {
  const priority: PaneStatus[] = ["Dead", "Waiting", "Working", "Idle"];
  let worst: PaneStatus | null = null;
  for (const pid of paneIds) {
    const s = statuses[pid];
    if (!s) continue;
    if (!worst || priority.indexOf(s) < priority.indexOf(worst)) {
      worst = s;
    }
  }
  return worst;
}
