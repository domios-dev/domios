import { RotateCw } from "lucide-react";
import type { AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import { formatClock, formatStamp } from "../model/format";

export function Header({
  snapshot,
  filterCount,
  refreshing,
  onRefresh,
}: {
  snapshot: AnalyticsSnapshot | null;
  filterCount: number;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  const since = snapshot?.recording.first_recorded_at ?? null;
  return (
    <header className="analytics-head">
      <div className="analytics-head-main">
        <h1 id="analytics-title" className="analytics-title">
          Analytics
        </h1>
        <p className="analytics-scope" data-testid="analytics-scope">
          <span className="ct-small-pill analytics-pill-accent">This machine</span>
          <span>
            {filterCount > 0
              ? `${filterCount} ${filterCount === 1 ? "filter" : "filters"} applied`
              : "Every workspace, team and plan"}
          </span>
          {since !== null && <span>Recorded since {formatStamp(since)}</span>}
          <span>Other machines on your mesh aren't included</span>
        </p>
      </div>
      <div className="analytics-head-side">
        {snapshot && (
          <span className="mc-board-caption" data-testid="analytics-as-of">
            as of {formatClock(snapshot.as_of * 1000)}
          </span>
        )}
        <button
          type="button"
          data-testid="analytics-refresh"
          className="mc-board-chip analytics-icon-btn"
          aria-label="Refresh"
          aria-busy={refreshing}
          onClick={onRefresh}
        >
          <RotateCw size={14} aria-hidden />
        </button>
      </div>
    </header>
  );
}
