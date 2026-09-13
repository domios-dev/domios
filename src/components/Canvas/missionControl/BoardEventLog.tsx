import { useCallback, useId, useMemo, useState } from "react";
import { BoardPanel, BoardPanelSection } from "./BoardPanel";
import { RefText, type BoardRefs } from "./BoardRefText";
import {
  EVENT_LOG_PAGE,
  EVENT_PART_SEPARATOR,
  EVENT_REVEAL_HIDE_LABEL,
  EVENT_REVEAL_SHOW_LABEL,
  eventLogCaption,
  eventLogRows,
  pagerPages,
  planEventPager,
  type EventPager,
  type EventRow,
} from "./missionControlBoardModel";
import type { EventLog } from "./missionControlState";
import type { MilestoneKind } from "../types";

export interface BoardEventLogProps {
  log: EventLog;
  milestoneKinds: readonly MilestoneKind[];
  refs?: BoardRefs;
  /** Page source; defaults to the plan's stored log, newest first. */
  pager?: EventPager;
}

interface LogRowProps {
  event: EventRow;
  expanded: boolean;
  refs: BoardRefs;
  onToggle: (key: string) => void;
}

const NO_REFS: BoardRefs = new Map();

function LogRow({ event, expanded, refs, onToggle }: LogRowProps) {
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
          <RefText text={open ? event.body : event.preview} refs={refs} />
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

export function BoardEventLog({ log, milestoneKinds, refs = NO_REFS, pager }: BoardEventLogProps) {
  const source = useMemo(() => pager ?? planEventPager(log), [pager, log]);
  const [offset, setOffset] = useState(0);
  const lastOffset = Math.max(0, Math.ceil(source.total / EVENT_LOG_PAGE) * EVENT_LOG_PAGE - EVENT_LOG_PAGE);
  const pageOffset = Math.min(offset, lastOffset);
  const rows = eventLogRows(
    { ...log, entries: source.page(pageOffset, EVENT_LOG_PAGE) },
    milestoneKinds,
    pageOffset,
  );
  const logId = useId();
  const [openKeys, setOpenKeys] = useState<readonly string[]>([]);
  const toggle = useCallback((key: string) => {
    setOpenKeys((keys) => (keys.includes(key) ? keys.filter((k) => k !== key) : [...keys, key]));
  }, []);

  return (
    <BoardPanel testId="mc-board-event-log" title="What happened" meta={eventLogCaption(log)}>
      <BoardPanelSection>
        {rows.length === 0 ? (
          <p data-testid="mc-board-event-log-empty" className="mc-board-note">
            No events yet.
          </p>
        ) : (
          <div id={logId} className="mc-board-log">
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
                  refs={refs}
                  onToggle={toggle}
                />
              ),
            )}
          </div>
        )}
        {source.total > EVENT_LOG_PAGE && (
          <nav aria-label="Event pages" data-testid="mc-board-event-log-pager" className="mc-board-log-pager">
            <button
              type="button"
              data-testid="mc-board-event-log-newer"
              className="mc-board-chip"
              aria-label="Newer events"
              aria-controls={logId}
              disabled={pageOffset === 0}
              onClick={() => setOffset(Math.max(0, pageOffset - EVENT_LOG_PAGE))}
            >
              ‹
            </button>
            {pagerPages(pageOffset / EVENT_LOG_PAGE + 1, lastOffset / EVENT_LOG_PAGE + 1).map((page, index) =>
              page === "gap" ? (
                <span key={`gap-${index}`} aria-hidden="true" className="mc-board-log-pager-gap">
                  …
                </span>
              ) : (
                <button
                  key={page}
                  type="button"
                  data-testid="mc-board-event-log-page"
                  className="mc-board-chip"
                  aria-controls={logId}
                  aria-current={page === pageOffset / EVENT_LOG_PAGE + 1 ? "page" : undefined}
                  onClick={() => setOffset((page - 1) * EVENT_LOG_PAGE)}
                >
                  {page}
                </button>
              ),
            )}
            <button
              type="button"
              data-testid="mc-board-event-log-older"
              className="mc-board-chip"
              aria-label="Older events"
              aria-controls={logId}
              disabled={pageOffset >= lastOffset}
              onClick={() => setOffset(Math.min(lastOffset, pageOffset + EVENT_LOG_PAGE))}
            >
              ›
            </button>
          </nav>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}
