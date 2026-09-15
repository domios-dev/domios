import { spendChartToneAttr, type SpendChartData, type SpendChartDay } from "./spendChart";

function dayReading(day: SpendChartDay, format: (value: number) => string): string {
  return day.coverage === "not_recorded" ? `${day.label} not recorded` : `${day.label} ${format(day.total)}`;
}

/** A bar per day, split into tone segments when the data carries a legend, which then sits under the bars. */
export function SpendChart({
  data,
  label,
  format,
  unit = "day",
  selectedStart = null,
  onSelectDay,
}: {
  data: SpendChartData;
  label: string;
  format: (value: number) => string;
  unit?: string;
  selectedStart?: number | null;
  /** Makes each bar a button that reports its day, for surfaces with no hover. */
  onSelectDay?: (start: number) => void;
}) {
  const peak = Math.max(1, ...data.days.filter((day) => day.coverage !== "not_recorded").map((day) => day.total));
  const summary = data.days.map((day) => dayReading(day, format)).join(", ");

  return (
    <div data-testid="mc-board-spend-chart" className="mc-board-chart">
      <div
        role={onSelectDay ? "group" : "img"}
        aria-label={`${label} by ${unit}: ${summary}`}
        className="mc-board-chart-bars"
      >
        {data.days.map((day) => {
          const notRecorded = day.coverage === "not_recorded";
          const reading = dayReading(day, format);
          const stack = (
            <div
              className="mc-board-chart-stack"
              data-coverage={day.coverage}
              title={reading}
              style={{ height: notRecorded ? "100%" : `${Math.round((day.total / peak) * 100)}%` }}
            >
              {!notRecorded &&
                day.segments.map((segment) => (
                  <span
                    key={segment.key}
                    data-testid="mc-board-spend-chart-segment"
                    data-tone={spendChartToneAttr(segment.tone)}
                    className="mc-board-chart-segment"
                    title={`${segment.label} ${format(segment.value)}`}
                    style={{ flexGrow: segment.value }}
                  />
                ))}
            </div>
          );
          return (
            <div
              key={day.start}
              data-testid="mc-board-spend-chart-day"
              data-today={day.today ? "true" : undefined}
              data-coverage={day.coverage}
              className="mc-board-chart-day"
            >
              {onSelectDay ? (
                <button
                  type="button"
                  data-testid="mc-board-spend-chart-select"
                  className="mc-board-chart-track mc-board-chart-hit"
                  aria-label={reading}
                  aria-pressed={selectedStart === day.start}
                  onClick={() => onSelectDay(day.start)}
                >
                  {stack}
                </button>
              ) : (
                <div className="mc-board-chart-track">{stack}</div>
              )}
              <span aria-hidden="true" className="mc-board-chart-label">
                {day.label}
                {day.note && <span className="mc-board-chart-note">{day.note}</span>}
              </span>
            </div>
          );
        })}
      </div>
      {data.legend.length > 0 && (
        <ul data-testid="mc-board-spend-legend" className="mc-board-chart-legend" role="list">
          {data.legend.map((series) => (
            <li key={series.key} data-testid="mc-board-spend-legend-item" className="mc-board-chart-legend-item">
              <span aria-hidden="true" className="mc-board-swatch" data-tone={spendChartToneAttr(series.tone)} />
              <span>{series.label}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
