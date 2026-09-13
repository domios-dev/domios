import { useState } from "react";
import { formatCompact } from "./missionControlBoardModel";
import {
  ALL_FILTER,
  SPEND_METRIC_LABELS,
  type SpendChartSource,
  type SpendMetric,
} from "./spendChart";

const METRICS: readonly SpendMetric[] = ["tokens", "points"];

function formatValue(metric: SpendMetric, value: number): string {
  return metric === "points" ? `${Math.round(value)}%` : formatCompact(value);
}

/** A light day-by-day spend chart: pick what to measure, then everything or one part of it. */
export function SpendChart({ source }: { source: SpendChartSource }) {
  const [chosen, setChosen] = useState<SpendMetric | null>(null);
  const [filter, setFilter] = useState(ALL_FILTER);
  const metric: SpendMetric = chosen && source.metrics[chosen] ? chosen : source.metrics.tokens ? "tokens" : "points";
  const filters = source.filters(metric);
  const activeFilter = filters.some((entry) => entry.key === filter) ? filter : ALL_FILTER;
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
              onClick={() => {
                setChosen(entry);
                setFilter(ALL_FILTER);
              }}
            >
              {SPEND_METRIC_LABELS[entry]}
            </button>
          ))}
        </div>
        {filters.length > 0 && (
          <div role="group" aria-label="Show" className="mc-board-chips">
            {filters.map((entry) => (
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
        <span data-testid="mc-board-spend-chart-scope" className="mc-board-chart-scope">
          {source.scope[metric]}
        </span>
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
                    data-tone={segment.tone % 4}
                    className="mc-board-chart-segment"
                    title={`${segment.label} ${formatValue(metric, segment.value)}`}
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
