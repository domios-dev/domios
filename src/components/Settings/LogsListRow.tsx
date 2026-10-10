import { useMemo, type CSSProperties } from "react";
import { ChevronRight } from "lucide-react";
import { timeAgo, formatTime } from "../../lib/formatters";
import type { AppLog } from "./LogsTab";
import { LEVEL_COLOR } from "./LogsTab";

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
  const color = LEVEL_COLOR[lvl as keyof typeof LEVEL_COLOR] ?? "var(--ct-muted)";
  const pretty = useMemo(() => {
    if (!row.context_json) return null;
    try {
      return JSON.stringify(JSON.parse(row.context_json), null, 2);
    } catch {
      return row.context_json;
    }
  }, [row.context_json]);

  return (
    <div className="ct-row ct-logs-row" data-expanded={expanded || undefined} data-testid="settings-log-row" data-log-id={row.id} data-log-level={lvl} style={{ "--ct-bar-pill-tone": color } as CSSProperties}>
      <button type="button" data-testid="settings-log-row-toggle" onClick={onToggle} aria-expanded={expanded} className="ct-logs-row__head">
        <ChevronRight size={12} aria-hidden="true" className="ct-logs-row__chevron" />
        <span className="ct-logs-row__time" title={new Date(row.ts).toISOString()}>{timeAgo(row.ts)}</span>
        <span className="ct-logs-row__level"><span className="ct-logs-dot" aria-hidden="true" />{lvl}</span>
        <span className="ct-logs-row__source">{row.source}</span>
        <span className="ct-logs-row__message">{row.message}</span>
      </button>
      {expanded && (
        <div className="ct-logs-row__detail">
          <div className="ct-logs-row__stamp">{formatTime(row.ts)} · {new Date(row.ts).toISOString()}</div>
          {pretty && <pre className="ct-logs-row__context">{pretty}</pre>}
        </div>
      )}
    </div>
  );
}
