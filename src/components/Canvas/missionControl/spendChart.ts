import type { AllowanceWeeks } from "./allowanceWeek";

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

/** What the spend chart can draw; weekly-limit points today, a team token series once it exists. */
export interface SpendChartSource {
  hasData: boolean;
  metrics: Record<SpendMetric, boolean>;
  filters: SpendChartFilter[];
  days(metric: SpendMetric, filter: string): SpendChartDay[];
}

function startOfToday(): number {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return today.getTime();
}

function dayLabel(start: number): string {
  return new Date(start).toLocaleDateString("en-US", { weekday: "short" });
}

/** Weekly-limit points spent per provider per calendar day, from usage history; it has no model split. */
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

  return {
    hasData: tracked.length > 0,
    metrics: { points: tracked.length > 0, tokens: false },
    filters:
      tracked.length > 1
        ? [{ key: ALL_FILTER, label: "All" }, ...tracked.map((provider) => ({ key: provider.key, label: provider.label }))]
        : [],
    days: (metric, filter) => (metric === "points" ? pointDays(filter) : []),
  };
}
