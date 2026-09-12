import { eventLogCaption, eventLogRows } from "./missionControlBoardModel";
import type { EventLog } from "./missionControlState";
import type { MilestoneKind } from "../types";

export interface BoardEventLogProps {
  log: EventLog;
  milestoneKinds: readonly MilestoneKind[];
}

export function BoardEventLog({ log, milestoneKinds }: BoardEventLogProps) {
  const rows = eventLogRows(log, milestoneKinds);

  return (
    <section data-testid="mc-board-event-log" className="ct-panel-card mc-board-card">
      <div className="mc-board-card-head">
        <h2>What happened</h2>
        <span className="mc-board-caption">{eventLogCaption(log)}</span>
      </div>
      {rows.length === 0 ? (
        <p data-testid="mc-board-event-log-empty" className="mc-board-note">
          No events yet.
        </p>
      ) : (
        <div className="mc-board-log">
          {rows.map((row) =>
            row.event === null ? (
              <span key={row.key} data-testid="mc-board-event-hour" className="mc-board-log-hour">
                {row.band}
              </span>
            ) : (
              <span key={row.key} data-testid="mc-board-event-row" className="mc-board-log-row">
                <span className="mc-board-log-time">{row.event.time}</span>
                <span
                  className={`mc-board-log-kind${row.event.strong ? " mc-board-log-kind-strong" : ""}`}
                >
                  {row.event.kindLabel}
                </span>
                <span data-testid="mc-board-event-author" className="mc-board-log-author">
                  {row.event.author}
                </span>
                <span
                  className={`mc-board-log-message${row.event.strong ? "" : " mc-board-log-message-quiet"}`}
                >
                  {row.event.taskName && (
                    <span className="mc-board-log-task">{row.event.taskName}</span>
                  )}
                  {row.event.text}
                  {row.event.runLabel && (
                    <span data-testid="mc-board-event-run" className="mc-board-log-run">
                      {row.event.runLabel}
                    </span>
                  )}
                </span>
              </span>
            ),
          )}
        </div>
      )}
    </section>
  );
}
