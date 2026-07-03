import type { TaskStatus } from "../types";
import {
  isPlanListSectionRow,
  isPlanListTaskRow,
  type PlanListRow,
  type PlanListVisibleRow,
} from "./buildPlanListTree";

export type PlanListStatusFilter = "all" | "active" | "done" | "blocked";

export const PLAN_LIST_STATUS_FILTERS: ReadonlyArray<{ key: PlanListStatusFilter; label: string }> = [
  { key: "all", label: "All" },
  { key: "active", label: "Active" },
  { key: "done", label: "Done" },
  { key: "blocked", label: "Blocked" },
];

function textHaystack(row: PlanListRow): string {
  return [row.title, row.focus, row.paneLabel, row.statusLabel, row.historicalExecutorSummary]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

export function formatRelativeAge(ts: number | undefined, now: number): string {
  if (!ts || ts <= 0) return "";
  const secs = Math.max(0, Math.floor((now - ts) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks}w`;
  return `${Math.floor(days / 30)}mo`;
}

export function taskRowMatchesStatus(row: PlanListRow, filter: PlanListStatusFilter): boolean {
  if (!isPlanListTaskRow(row)) return true;
  const status = row.status as TaskStatus | undefined;
  if (filter === "all") return true;
  // Parked work is deferred, not active — fold it out of the Active filter.
  if (filter === "active") return status !== "done" && status !== "cancelled" && status !== "parked";
  if (filter === "done") return status === "done";
  if (filter === "blocked") return status === "blocked";
  return true;
}

export function rowMatchesTextQuery(row: PlanListRow, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return textHaystack(row).includes(q);
}

export function defaultSectionExpanded(row: PlanListRow): boolean {
  if (!isPlanListSectionRow(row)) return true;
  return !row.allTasksDone;
}

export function resolveSectionExpanded(
  row: PlanListRow,
  expansionOverrides: ReadonlyMap<string, boolean>,
): boolean {
  const override = expansionOverrides.get(row.id);
  if (override != null) return override;
  return defaultSectionExpanded(row);
}

function rowPassesFilters(
  row: PlanListRow,
  statusFilter: PlanListStatusFilter,
  textQuery: string,
): boolean {
  if (isPlanListTaskRow(row)) {
    return taskRowMatchesStatus(row, statusFilter) && rowMatchesTextQuery(row, textQuery);
  }
  if (textQuery.trim()) return rowMatchesTextQuery(row, textQuery);
  return true;
}

function subtreeHasVisibleTask(
  rows: readonly PlanListRow[],
  startIdx: number,
  sectionDepth: number,
  statusFilter: PlanListStatusFilter,
  textQuery: string,
): boolean {
  for (let i = startIdx + 1; i < rows.length; i++) {
    const row = rows[i];
    if (row.depth <= sectionDepth && isPlanListSectionRow(row)) break;
    if (isPlanListTaskRow(row) && rowPassesFilters(row, statusFilter, textQuery)) return true;
  }
  return false;
}

export function buildVisiblePlanListRows(
  rows: readonly PlanListRow[],
  opts: {
    statusFilter: PlanListStatusFilter;
    textQuery: string;
    expansionOverrides: ReadonlyMap<string, boolean>;
  },
): PlanListVisibleRow[] {
  const { statusFilter, textQuery, expansionOverrides } = opts;
  const out: PlanListVisibleRow[] = [];
  let hiddenBelowDepth = -1;

  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    if (hiddenBelowDepth >= 0) {
      if (row.depth <= hiddenBelowDepth && isPlanListSectionRow(row)) {
        hiddenBelowDepth = -1;
      } else if (row.depth > hiddenBelowDepth) {
        continue;
      }
    }

    if (isPlanListSectionRow(row)) {
      const hasVisibleTasks = subtreeHasVisibleTask(rows, i, row.depth, statusFilter, textQuery);
      const sectionMatchesText = rowMatchesTextQuery(row, textQuery);
      if (textQuery.trim() && !hasVisibleTasks && !sectionMatchesText) continue;

      const expanded = resolveSectionExpanded(row, expansionOverrides);
      out.push({ type: "row", row });

      if (!expanded) {
        const doneCount = row.doneCount ?? 0;
        if (doneCount > 0 && row.allTasksDone) {
          out.push({ type: "done-rollup", sectionRow: row, doneCount });
        }
        hiddenBelowDepth = row.depth;
      }
      continue;
    }

    if (!rowPassesFilters(row, statusFilter, textQuery)) continue;
    out.push({ type: "row", row });
  }

  return out;
}
