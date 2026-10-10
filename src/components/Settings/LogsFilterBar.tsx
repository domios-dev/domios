import type { CSSProperties } from "react";
import type { LogLevel, RangeKey } from "./LogsTab";
import { ALL_LEVELS, LEVEL_COLOR, RANGE_OPTIONS } from "./LogsTab";
import { Segmented } from "../shared/Segmented";

export function LogsFilterBar({
  levels,
  onToggleLevel,
  source,
  onSourceChange,
  sources,
  range,
  onRangeChange,
}: {
  levels: Set<LogLevel>;
  onToggleLevel: (l: LogLevel) => void;
  source: string;
  onSourceChange: (s: string) => void;
  sources: string[];
  range: RangeKey;
  onRangeChange: (r: RangeKey) => void;
}) {
  return (
    <div role="group" aria-label="Log filters" className="ct-logs-filters" data-testid="settings-logs-filter-bar">
      <div role="group" aria-label="Log levels" className="flex items-center gap-1.5">
        {ALL_LEVELS.map((l) => (
          <button
            type="button"
            key={l}
            className="ct-bar-pill ct-logs-level"
            style={{ "--ct-bar-pill-tone": LEVEL_COLOR[l] } as CSSProperties}
            data-testid="settings-logs-level-filter"
            data-log-level={l}
            aria-pressed={levels.has(l)}
            onClick={() => onToggleLevel(l)}
          >
            <span className="ct-logs-dot" aria-hidden="true" />
            {l}
          </button>
        ))}
      </div>
      <input
        type="text"
        data-testid="settings-logs-source-filter"
        list="logs-source-list"
        value={source}
        onChange={(e) => onSourceChange(e.target.value)}
        placeholder="All sources"
        aria-label="Filter by source"
        className="ct-field ct-logs-source"
      />
      <datalist id="logs-source-list">
        {sources.map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
      <Segmented
        aria-label="Time range"
        data-testid="settings-logs-range-filter"
        value={range}
        height={28}
        options={RANGE_OPTIONS.map((opt) => ({ value: opt.key, label: opt.label }))}
        onChange={onRangeChange}
      />
    </div>
  );
}
