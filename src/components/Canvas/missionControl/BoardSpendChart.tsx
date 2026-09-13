import { useState } from "react";
import {
  ALL_FILTER,
  SPEND_METRIC_LABELS,
  type SpendChartSource,
  type SpendMetric,
} from "./spendChart";

const METRICS: readonly SpendMetric[] = ["points", "tokens"];

function formatValue(metric: SpendMetric, value: number): string {
  return metric === "points" ? `${Math.round(value)}%` : String(Math.round(value));
}

/** A light day-by-day spend chart: pick what to measure, then everyone or one source. */
export function SpendChart({ source }: { source: SpendChartSource }) {
  const [metric, setMetric] = useState<SpendMetric>("points");
  const [filter, setFilter] = useState(ALL_FILTER);
  const activeFilter = source.filters.some((entry) => entry.key === filter) ? filter : ALL_FILTER;
  const days = source.days(metric, activeFilter);
  const peak = Math.max(1, ...days.map((day) => day.total));
  const summary = days.map((day) => `${day.label} ${formatValue(metric, day.total)}`).join(", ");

  return (
    <div data-testid="mc-board-spend-chart" className="mc-board-chart">
      <div className="mc-board-chart-controls">
        <div role="group" aria-label="Measure" className="mc-board-chips">
          {METRICS.map((entry) => (
            <button
              key={entry}
              type="button"
              data-testid={`mc-board-spend-metric-${entry}`}
              className="mc-board-chip"
              aria-pressed={metric === entry}
              disabled={!source.metrics[entry]}
              onClick={() => setMetric(entry)}
            >
              {SPEND_METRIC_LABELS[entry]}
            </button>
          ))}
        </div>
        {source.filters.length > 0 && (
          <div role="group" aria-label="Show" className="mc-board-chips">
            {source.filters.map((entry) => (
              <button
                key={entry.key}
                type="button"
                data-testid="mc-board-spend-filter"
                data-filter={entry.key}
                className="mc-board-chip"
                aria-pressed={activeFilter === entry.key}
                onClick={() => setFilter(entry.key)}
              >
                {entry.label}
              </button>
            ))}
          </div>
        )}
      </div>
      <div
        role="img"
        aria-label={`${SPEND_METRIC_LABELS[metric]} by day: ${summary}`}
        className="mc-board-chart-bars"
      >
        {days.map((day) => (
          <div
            key={day.start}
            data-testid="mc-board-spend-chart-day"
            data-today={day.today ? "true" : undefined}
            className="mc-board-chart-day"
          >
            <div className="mc-board-chart-track">
              <div
                className="mc-board-chart-stack"
                title={`${day.label} ${formatValue(metric, day.total)}`}
                style={{ height: `${Math.round((day.total / peak) * 100)}%` }}
              >
                {day.segments.map((segment) => (
                  <span
                    key={segment.key}
                    data-testid="mc-board-spend-chart-segment"
                    data-tone={segment.tone}
                    className="mc-board-chart-segment"
                    style={{ flexGrow: segment.value }}
                  />
                ))}
              </div>
            </div>
            <span aria-hidden="true" className="mc-board-chart-label">
              {day.label}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
