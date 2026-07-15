import type { UsageSnapshot } from "../../types/usage";

export interface VisibleUsageMetric {
  key: "session" | "weekly";
  label: "Session" | "Weekly";
  pct: number;
  resetsAt: number | null;
}

export function visibleUsageMetrics(
  snapshot: Pick<UsageSnapshot, "session_pct" | "session_resets_at" | "weekly_pct" | "weekly_resets_at">,
): VisibleUsageMetric[] {
  const metrics: VisibleUsageMetric[] = [];
  if (snapshot.session_pct != null) {
    metrics.push({
      key: "session",
      label: "Session",
      pct: snapshot.session_pct,
      resetsAt: snapshot.session_resets_at,
    });
  }
  if (snapshot.weekly_pct != null) {
    metrics.push({
      key: "weekly",
      label: "Weekly",
      pct: snapshot.weekly_pct,
      resetsAt: snapshot.weekly_resets_at,
    });
  }
  return metrics;
}

export function usageMetricsAriaLabel(metrics: VisibleUsageMetric[]): string {
  return `usage ${metrics.map((metric) => `${metric.key} ${Math.round(metric.pct)}%`).join(" ")}`.trim();
}
