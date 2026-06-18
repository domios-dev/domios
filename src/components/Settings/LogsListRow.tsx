import { useMemo } from "react";
import { ChevronRight } from "lucide-react";
import { timeAgo, formatTime } from "../../lib/formatters";
import { SettingsPill } from "./shared";
import type { AppLog } from "./LogsTab";
import { LEVEL_TONE } from "./LogsTab";

export function LogsListRow({
  row,
  expanded,
  onToggle,
}: {
  row: AppLog;
  expanded: boolean;
  onToggle: () => void;
}) {
  const lvl = row.level.toLowerCase();
  const tone = LEVEL_TONE[lvl] ?? "muted";
  const pretty = useMemo(() => {
    if (!row.context_json) return null;
    try {
      return JSON.stringify(JSON.parse(row.context_json), null, 2);
    } catch {
      return row.context_json;
    }
  }, [row.context_json]);

  return (
    <div className="px-3 py-1.5 hover:bg-[var(--ct-hover)]/40 transition-colors" data-testid="settings-log-row" data-log-id={row.id} data-log-level={lvl}>
      <button
        data-testid="settings-log-row-toggle"
        onClick={onToggle}
        className="w-full flex items-center gap-2 text-left text-[11px]"
      >
        <ChevronRight
          size={11}
          className={`shrink-0 text-[var(--ct-muted)] transition-transform ${expanded ? "rotate-90" : ""}`}
        />
        <span
          className="shrink-0 text-[10px] text-[var(--ct-muted-fg)] tabular-nums w-[64px]"
          title={new Date(row.ts).toISOString()}
        >
          {timeAgo(row.ts)}
        </span>
        <span className="shrink-0">
          <SettingsPill tone={tone}>{lvl}</SettingsPill>
        </span>
        <span className="shrink-0 font-mono text-[10px] text-[var(--ct-muted)] max-w-[160px] truncate">
          {row.source}
        </span>
        <span
          className={`flex-1 min-w-0 text-[var(--ct-fg)] ${expanded ? "whitespace-pre-wrap break-words" : "truncate"}`}
        >
          {row.message}
        </span>
      </button>
      {expanded && (
        <div className="mt-1.5 ml-[13px] space-y-1.5">
          <div className="text-[10px] text-[var(--ct-muted)] font-mono">
            {formatTime(row.ts)} · {new Date(row.ts).toISOString()}
          </div>
          {pretty && (
            <pre className="text-[10px] font-mono leading-relaxed bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded p-2 text-[var(--ct-fg)] overflow-x-auto whitespace-pre-wrap break-words">
{pretty}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
