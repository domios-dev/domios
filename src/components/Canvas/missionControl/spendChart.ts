import type { AllowanceWeeks } from "./allowanceWeek";
import { SPEND_SCOPE_CAPTION } from "./missionControlBoardModel";

export type SpendMetric = "points" | "tokens";

export const SPEND_METRIC_LABELS: Record<SpendMetric, string> = {
  points: "limit %",
  tokens: "tokens",
};

export const ALL_FILTER = "all";

const CHART_DAYS = 7;

export interface SpendChartFilter {
  key: string;
  label: string;
}

export interface SpendChartSegment {
  key: string;
  label: string;
  tone: number;
  value: number;
}

export interface SpendChartDay {
  start: number;
  label: string;
  today: boolean;
  segments: SpendChartSegment[];
  total: number;
}

/** What the spend chart can draw per measure, where each measure comes from, and how it can be narrowed. */
export interface SpendChartSource {
  hasData: boolean;
  metrics: Record<SpendMetric, boolean>;
  /** A word or two naming where a measure is counted. */
  scope: Record<SpendMetric, string>;
  filters(metric: SpendMetric): SpendChartFilter[];
  days(metric: SpendMetric, filter: string): SpendChartDay[];
}

export function startOfToday(): number {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return today.getTime();
}

export function dayLabel(start: number): string {
  return new Date(start).toLocaleDateString("en-US", { weekday: "short" });
}

/** Weekly-limit points spent per provider per calendar day, from usage history; it has no model or team split. */
export function allowanceChartSource(
  weeks: AllowanceWeeks,
  providers: ReadonlyArray<{ key: string; label: string; tone: number }>,
): SpendChartSource {
  const tracked = providers.filter((provider) => (weeks[provider.key] ?? []).some((day) => day.pct != null));
  const today = startOfToday();
  const starts = [
    ...new Set(
      tracked.flatMap((provider) => (weeks[provider.key] ?? []).filter((day) => day.start <= today).map((day) => day.start)),
    ),
  ]
    .sort((a, b) => a - b)
    .slice(-CHART_DAYS);

  const pointDays = (filter: string): SpendChartDay[] =>
    starts.map((start) => {
      const segments = tracked
        .filter((provider) => filter === ALL_FILTER || provider.key === filter)
        .flatMap((provider) => {
          const pct = (weeks[provider.key] ?? []).find((day) => day.start === start)?.pct ?? null;
          return pct != null && pct > 0 ? [{ key: provider.key, label: provider.label, tone: provider.tone, value: pct }] : [];
        });
      return {
        start,
        label: dayLabel(start),
        today: start === today,
        segments,
        total: segments.reduce((sum, segment) => sum + segment.value, 0),
      };
    });

  const providerFilters =
    tracked.length > 1
      ? [{ key: ALL_FILTER, label: "All" }, ...tracked.map((provider) => ({ key: provider.key, label: provider.label }))]
      : [];

  return {
    hasData: tracked.length > 0,
    metrics: { points: tracked.length > 0, tokens: false },
    scope: { points: SPEND_SCOPE_CAPTION, tokens: "" },
    filters: (metric) => (metric === "points" ? providerFilters : []),
    days: (metric, filter) => (metric === "points" ? pointDays(filter) : []),
  };
}

/** Team tokens where the team source has them, limit points from usage history otherwise. */
export function combineChartSources(tokens: SpendChartSource | null, points: SpendChartSource): SpendChartSource {
  if (!tokens) return points;
  const pick = (metric: SpendMetric) => (metric === "tokens" ? tokens : points);
  return {
    hasData: tokens.hasData || points.hasData,
    metrics: { tokens: tokens.metrics.tokens, points: points.metrics.points },
    scope: { tokens: tokens.scope.tokens, points: points.scope.points },
    filters: (metric) => pick(metric).filters(metric),
    days: (metric, filter) => pick(metric).days(metric, filter),
  };
}
