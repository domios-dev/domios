import type { VisibleUsageMetric } from "./metrics";

export function UsageMetricGlyph({ metric, state, separated }: {
  metric: Pick<VisibleUsageMetric, "key" | "pct">;
  state: "neutral" | "warn" | "danger";
  separated: boolean;
}) {
  return <>
    {separated && <span className="sep" />}
    <span className={`glyph ${state}`}>
      <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
        {metric.key === "session" ? <><circle cx="6" cy="6" r="4.6" /><path d="M6 3.4V6l1.7 1.1" /></> : <path d="M1 7.2 3 5l2.2 2.2L7.4 4l3.4 3.4" />}
      </svg>
      <span className="pct">{Math.round(metric.pct)}%</span>
    </span>
  </>;
}
