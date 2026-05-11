import type { LogLevel, RangeKey } from "./LogsTab";
import { ALL_LEVELS, LEVEL_TONE, RANGE_OPTIONS } from "./LogsTab";

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
    <div className="flex flex-wrap items-center gap-3">
      <div className="flex items-center gap-1.5">
        {ALL_LEVELS.map((l) => {
          const active = levels.has(l);
          const tone = LEVEL_TONE[l];
          const activeClasses =
            tone === "green"
              ? "bg-[var(--ct-green)]/15 text-[var(--ct-green)] border-[var(--ct-green)]/30"
              : tone === "amber"
                ? "bg-[var(--ct-yellow)]/15 text-[var(--ct-yellow)] border-[var(--ct-yellow)]/30"
                : "bg-[var(--ct-red)]/15 text-[var(--ct-red)] border-[var(--ct-red)]/30";
          return (
            <button
              key={l}
              onClick={() => onToggleLevel(l)}
              className={`px-2 py-0.5 rounded-full text-[10px] uppercase tracking-wider font-medium border transition-colors ${
                active
                  ? activeClasses
                  : "bg-transparent text-[var(--ct-muted-fg)] border-[var(--ct-border)] hover:text-[var(--ct-fg)]"
              }`}
            >
              {l}
            </button>
          );
        })}
      </div>

      <div className="flex-1 min-w-[160px]">
        <input
          type="text"
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

      <div className="inline-flex items-center rounded-md border border-[var(--ct-border)] overflow-hidden">
        {RANGE_OPTIONS.map((opt) => {
          const active = range === opt.key;
          return (
            <button
              key={opt.key}
              onClick={() => onRangeChange(opt.key)}
              className={`px-2 py-1 text-[10px] uppercase tracking-wider transition-colors ${
                active
                  ? "bg-[var(--ct-accent)]/20 text-[var(--ct-accent)]"
                  : "text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)]"
              }`}
            >
              {opt.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
