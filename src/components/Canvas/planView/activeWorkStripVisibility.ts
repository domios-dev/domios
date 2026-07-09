import type { ActiveWorkEntry } from "./activeWork";
import type { PlanListVisibleRow } from "./buildPlanListTree";

export function visiblePlanListTaskIds(visibleRows: readonly PlanListVisibleRow[]): ReadonlySet<string> {
  const ids = new Set<string>();
  for (const entry of visibleRows) {
    if (entry.type === "row" && entry.row.taskId) ids.add(entry.row.taskId);
  }
  return ids;
}

export function shouldShowActiveWorkStrip(
  entries: readonly ActiveWorkEntry[],
  visibleRows: readonly PlanListVisibleRow[],
): boolean {
  if (entries.length === 0) return false;
  const visible = visiblePlanListTaskIds(visibleRows);
  return entries.some((entry) => !visible.has(entry.task.id));
}

export function activeWorkEntriesForStrip(
  entries: readonly ActiveWorkEntry[],
  visibleRows: readonly PlanListVisibleRow[],
): readonly ActiveWorkEntry[] {
  if (!shouldShowActiveWorkStrip(entries, visibleRows)) return [];
  const visible = visiblePlanListTaskIds(visibleRows);
  return entries.filter((entry) => !visible.has(entry.task.id));
}
