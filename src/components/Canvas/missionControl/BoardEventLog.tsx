import { useCallback, useState } from "react";
import {
  EVENT_PART_SEPARATOR,
  EVENT_REVEAL_HIDE_LABEL,
  EVENT_REVEAL_SHOW_LABEL,
  eventLogCaption,
  eventLogRows,
  type EventRow,
} from "./missionControlBoardModel";
import type { EventLog } from "./missionControlState";
import type { MilestoneKind } from "../types";

export interface BoardEventLogProps {
  log: EventLog;
  milestoneKinds: readonly MilestoneKind[];
}

interface LogRowProps {
  event: EventRow;
  expanded: boolean;
  onToggle: (key: string) => void;
}

function LogRow({ event, expanded, onToggle }: LogRowProps) {
  const bodyId = `mc-board-event-body-${event.key}`;
  const open = expanded && event.expandable;
  return (
    <span data-testid="mc-board-event-row" className="mc-board-log-row">
      <span className="mc-board-log-time">{event.time}</span>
      <span className={`mc-board-log-kind${event.strong ? " mc-board-log-kind-strong" : ""}`}>
        {event.kindLabel}
      </span>
      <span data-testid="mc-board-event-author" className="mc-board-log-author">
        {event.author}
      </span>
      <span
        className={`mc-board-log-message${event.strong ? "" : " mc-board-log-message-quiet"}${
          open ? " mc-board-log-message-open" : ""
        }`}
      >
        {event.taskName && (
          <>
            <span data-testid="mc-board-event-task" className="mc-board-log-task">
              {event.taskName}
            </span>
            <span className="mc-board-log-sep">{EVENT_PART_SEPARATOR}</span>
          </>
        )}
        <span
          id={bodyId}
          data-testid="mc-board-event-body"
          data-expanded={open ? "true" : "false"}
          className="mc-board-log-body"
        >
          {open ? event.body : event.preview}
        </span>
        {event.runLabel && (
          <span data-testid="mc-board-event-run" className="mc-board-log-run">
            {event.runLabel}
          </span>
        )}
        {event.expandable && (
          <button
            type="button"
            data-testid="mc-board-event-reveal"
            className="mc-board-log-more"
            aria-expanded={open}
            aria-controls={bodyId}
            aria-label={open ? EVENT_REVEAL_HIDE_LABEL : EVENT_REVEAL_SHOW_LABEL}
            onClick={() => onToggle(event.key)}
          >
            {open ? "Hide" : "Show full"}
          </button>
        )}
      </span>
    </span>
  );
}

export function BoardEventLog({ log, milestoneKinds }: BoardEventLogProps) {
  const rows = eventLogRows(log, milestoneKinds);
  const [openKeys, setOpenKeys] = useState<readonly string[]>([]);
  const toggle = useCallback((key: string) => {
    setOpenKeys((keys) => (keys.includes(key) ? keys.filter((k) => k !== key) : [...keys, key]));
  }, []);

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
              <LogRow
                key={row.key}
                event={row.event}
                expanded={openKeys.includes(row.event.key)}
                onToggle={toggle}
              />
            ),
          )}
        </div>
      )}
    </section>
  );
}
