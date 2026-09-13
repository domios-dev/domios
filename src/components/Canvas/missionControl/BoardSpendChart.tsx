import type { SpendChartData } from "./spendChart";

/** A bar per day, split into tone segments when the data carries a legend, which then sits under the bars. */
export function SpendChart({
  data,
  label,
  format,
}: {
  data: SpendChartData;
  label: string;
  format: (value: number) => string;
}) {
  const peak = Math.max(1, ...data.days.map((day) => day.total));
  const summary = data.days.map((day) => `${day.label} ${format(day.total)}`).join(", ");

  return (
    <div data-testid="mc-board-spend-chart" className="mc-board-chart">
      <div role="img" aria-label={`${label} by day: ${summary}`} className="mc-board-chart-bars">
        {data.days.map((day) => (
          <div
            key={day.start}
            data-testid="mc-board-spend-chart-day"
            data-today={day.today ? "true" : undefined}
            className="mc-board-chart-day"
          >
            <div className="mc-board-chart-track">
              <div
                className="mc-board-chart-stack"
                title={`${day.label} ${format(day.total)}`}
                style={{ height: `${Math.round((day.total / peak) * 100)}%` }}
              >
                {day.segments.map((segment) => (
                  <span
                    key={segment.key}
                    data-testid="mc-board-spend-chart-segment"
                    data-tone={segment.tone % 4}
                    className="mc-board-chart-segment"
                    title={`${segment.label} ${format(segment.value)}`}
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
      {data.legend.length > 0 && (
        <ul data-testid="mc-board-spend-legend" className="mc-board-chart-legend" role="list">
          {data.legend.map((series) => (
            <li key={series.key} data-testid="mc-board-spend-legend-item" className="mc-board-chart-legend-item">
              <span aria-hidden="true" className="mc-board-swatch" data-tone={series.tone % 4} />
              <span>{series.label}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
