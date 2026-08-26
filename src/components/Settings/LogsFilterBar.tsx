import type { LogLevel, RangeKey } from "./LogsTab";
import { ALL_LEVELS, LEVEL_TONE, RANGE_OPTIONS } from "./LogsTab";
import { Button } from "../shared/Button";

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
    <div
      role="group"
      aria-label="Log filters"
      className="flex flex-wrap items-center gap-3"
      data-testid="settings-logs-filter-bar"
    >
      <div role="group" aria-label="Log levels" className="flex items-center gap-1.5">
        {ALL_LEVELS.map((l) => {
          const active = levels.has(l);
          const tone = LEVEL_TONE[l];
          return (
            <Button
              variant={active ? (tone === "red" ? "danger" : "accent") : "ghost"}
              size="compact"
              key={l}
              data-testid="settings-logs-level-filter"
              data-log-level={l}
              aria-pressed={active}
              onClick={() => onToggleLevel(l)}
              className="text-[10px] uppercase tracking-wider"
            >
              {l}
            </Button>
          );
        })}
      </div>

      <div className="flex-1 min-w-[160px]">
        <input
          type="text"
          data-testid="settings-logs-source-filter"
          list="logs-source-list"
          value={source}
          onChange={(e) => onSourceChange(e.target.value)}
          placeholder="All sources"
          className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[11px] text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
        />
        <datalist id="logs-source-list">
          {sources.map((s) => (
            <option key={s} value={s} />
          ))}
        </datalist>
      </div>

      <div
        role="group"
        aria-label="Time range"
        className="inline-flex items-center rounded-md border border-[var(--ct-border)] overflow-hidden"
      >
        {RANGE_OPTIONS.map((opt) => {
          const active = range === opt.key;
          return (
            <Button
              variant={active ? "accent" : "ghost"}
              size="compact"
              key={opt.key}
              data-testid="settings-logs-range-filter"
              data-log-range={opt.key}
              aria-pressed={active}
              onClick={() => onRangeChange(opt.key)}
              className="rounded-none border-0 text-[10px] uppercase tracking-wider"
            >
              {opt.label}
            </Button>
          );
        })}
      </div>
    </div>
  );
}
