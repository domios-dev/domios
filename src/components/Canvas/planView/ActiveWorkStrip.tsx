import { TASK_STATUS_COLORS } from "../settings";
import type { ActiveWorkEntry } from "./activeWork";
import { displayTaskTitle } from "./activeWork";

export interface ActiveWorkStripProps {
  entries: readonly ActiveWorkEntry[];
  onOpenTask: (entry: ActiveWorkEntry) => void;
}

function statusLabel(status: ActiveWorkEntry["status"]): string {
  return status.replace(/_/g, " ");
}

export function ActiveWorkStrip({ entries, onOpenTask }: ActiveWorkStripProps) {
  if (entries.length === 0) return null;

  return (
    <div data-testid="active-work-strip" className="active-work-strip" role="list" aria-label="In flight">
      <div className="active-work-strip-label">In flight</div>
      <div className="active-work-strip-rows">
        {entries.map((entry) => {
          const color = TASK_STATUS_COLORS[entry.status];
          return (
            <button
              key={`${entry.planId}::${entry.task.id}`}
              type="button"
              role="listitem"
              data-testid="active-work-strip-row"
              data-task-id={entry.task.id}
              data-plan-id={entry.planId}
              data-stale={entry.stale ? "true" : "false"}
              className={`active-work-strip-row${entry.stale ? " active-work-strip-row-stale" : ""}`}
              title={`${displayTaskTitle(entry.task)} · ${entry.planTitle}`}
              onClick={() => onOpenTask(entry)}
            >
              <span
                className="active-work-strip-dot"
                style={{ background: color }}
                aria-hidden
              />
              <span className="active-work-strip-status">{statusLabel(entry.status)}</span>
              <span className="active-work-strip-name">{displayTaskTitle(entry.task)}</span>
              {entry.executorLabel && (
                <span className="active-work-strip-executor">{entry.executorLabel}</span>
              )}
              {entry.freshnessLabel && (
                <span className="active-work-strip-age">{entry.freshnessLabel}</span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
