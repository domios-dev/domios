import { useState } from "react";
import { formatCompact } from "./missionControlBoardModel";
import { ALL_FILTER, type SpendChartSource } from "./spendChart";

/** The team's new input per day as stacked bars, one segment per plan, narrowed to one model on request. */
export function SpendChart({ source }: { source: SpendChartSource }) {
  const [filter, setFilter] = useState(ALL_FILTER);
  const activeFilter = source.filters.some((entry) => entry.key === filter) ? filter : ALL_FILTER;
  const days = source.days(activeFilter);
  const peak = Math.max(1, ...days.map((day) => day.total));
  const summary = days.map((day) => `${day.label} ${formatCompact(day.total)}`).join(", ");

  return (
    <div data-testid="mc-board-spend-chart" className="mc-board-chart">
      {source.filters.length > 0 && (
        <div role="group" aria-label="Model" className="mc-board-chips">
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
      <div role="img" aria-label={`new input by day: ${summary}`} className="mc-board-chart-bars">
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
                title={`${day.label} ${formatCompact(day.total)}`}
                style={{ height: `${Math.round((day.total / peak) * 100)}%` }}
              >
                {day.segments.map((segment) => (
                  <span
                    key={segment.key}
                    data-testid="mc-board-spend-chart-segment"
                    data-tone={segment.tone % 4}
                    className="mc-board-chart-segment"
                    title={`${segment.label} ${formatCompact(segment.value)}`}
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
