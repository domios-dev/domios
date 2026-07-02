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
  return [row.title, row.focus, row.paneLabel, row.statusLabel, row.historicalExecutorSummary, row.meta]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

export function taskRowMatchesStatus(row: PlanListRow, filter: PlanListStatusFilter): boolean {
  if (!isPlanListTaskRow(row)) return true;
  const status = row.status as TaskStatus | undefined;
  if (filter === "all") return true;
  if (filter === "active") return status !== "done" && status !== "cancelled";
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
  if (row.kind === "spec-section") return true;
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
    if (row.depth <= sectionDepth && (isPlanListSectionRow(row) || row.kind === "spec-root" || row.kind === "plan-root")) {
      break;
    }
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
      if (row.depth <= hiddenBelowDepth && (isPlanListSectionRow(row) || row.kind === "spec-root" || row.kind === "plan-root")) {
        hiddenBelowDepth = -1;
      } else if (row.depth > hiddenBelowDepth) {
        continue;
      }
    }

    if (row.kind === "spec-root" || row.kind === "plan-root") {
      out.push({ type: "row", row });
      continue;
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

export function groupPlanListRowsByPlan(
  rows: readonly PlanListRow[],
): Array<{ planId: string; rows: PlanListRow[] }> {
  const map = new Map<string, PlanListRow[]>();
  const order: string[] = [];
  for (const row of rows) {
    if (!map.has(row.planId)) {
      map.set(row.planId, []);
      order.push(row.planId);
    }
    map.get(row.planId)!.push(row);
  }
  return order.map((planId) => ({ planId, rows: map.get(planId)! }));
}
