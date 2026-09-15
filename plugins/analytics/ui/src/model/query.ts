import type { AnalyticsGroupBy, AnalyticsModelKey, AnalyticsQuery } from "@codeterm/plugin-sdk";
import type { LocalRange } from "./range";

export interface FilterState {
  providers: string[];
  models: AnalyticsModelKey[];
  workspaces: Array<string | null>;
  teams: Array<string | null>;
  plans: Array<string | null>;
}

export type FilterGroupId = keyof FilterState;

export const NO_FILTERS: FilterState = { providers: [], models: [], workspaces: [], teams: [], plans: [] };

export const GROUP_BY_OPTIONS: Array<{ value: AnalyticsGroupBy; label: string }> = [
  { value: "provider_model", label: "Provider" },
  { value: "workspace", label: "Workspace" },
  { value: "team", label: "Team" },
  { value: "plan", label: "Plan" },
];

export function activeFilterCount(filters: FilterState): number {
  return (Object.keys(filters) as FilterGroupId[]).reduce((sum, group) => sum + filters[group].length, 0);
}

export function sameModel(a: AnalyticsModelKey, b: AnalyticsModelKey): boolean {
  return a.provider === b.provider && a.model === b.model;
}

export function toggled<T>(values: readonly T[], value: T, equal: (a: T, b: T) => boolean = Object.is): T[] {
  return values.some((entry) => equal(entry, value))
    ? values.filter((entry) => !equal(entry, value))
    : [...values, value];
}

/** Empty lists are left out so the host reads them as no filter. */
export function buildQuery(range: LocalRange, tz: string, groupBy: AnalyticsGroupBy, filters: FilterState): AnalyticsQuery {
  const chosen: NonNullable<AnalyticsQuery["filters"]> = {};
  for (const group of Object.keys(filters) as FilterGroupId[]) {
    if (filters[group].length > 0) (chosen as Record<string, unknown>)[group] = filters[group];
  }
  return {
    range: { from: range.from, to: range.to, tz },
    bucket: range.bucket,
    filters: chosen,
    group_by: groupBy,
    scope: { kind: "machine" },
  };
}
