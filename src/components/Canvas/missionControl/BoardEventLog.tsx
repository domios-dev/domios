import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { BoardPanel, BoardPanelSection } from "./BoardPanel";
import { RefLabel, RefText, type BoardRefs } from "./BoardRefText";
import {
  clipTaskName,
  EVENT_LOG_PAGE,
  EVENT_PART_SEPARATOR,
  EVENT_REVEAL_HIDE_LABEL,
  EVENT_REVEAL_SHOW_LABEL,
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
  /** The moment day labels are read against; defaults to the render time. */
  now?: number;
}

interface LogRowProps {
  event: EventRow;
  expanded: boolean;
  refs: BoardRefs;
  onToggle: (key: string) => void;
}

const NO_REFS: BoardRefs = new Map();

const clipTaskLabel = (label: string) => clipTaskName(label) ?? label;

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
              {event.taskId ? (
                <RefLabel id={event.taskId} refs={refs} fallback={event.taskName} format={clipTaskLabel} />
              ) : (
                event.taskName
              )}
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

export function BoardEventLog({ log, milestoneKinds, refs = NO_REFS, pager, now }: BoardEventLogProps) {
  const source = useMemo(() => pager ?? planEventPager(log), [pager, log]);
  const [pageIndex, setPageIndex] = useState(0);
  const lastIndex = Math.max(0, source.pageCount - 1);
  const index = Math.min(pageIndex, lastIndex);
  const entries = source.page(index);
  useEffect(() => {
    if (entries === null) source.load?.(index);
  }, [entries, index, source]);
  const rows = entries ? eventLogRows({ ...log, entries }, milestoneKinds, index * EVENT_LOG_PAGE, now) : [];
  const logId = useId();
  const [openKeys, setOpenKeys] = useState<readonly string[]>([]);
  const toggle = useCallback((key: string) => {
    setOpenKeys((keys) => (keys.includes(key) ? keys.filter((k) => k !== key) : [...keys, key]));
  }, []);

  return (
    <BoardPanel testId="mc-board-event-log" title="What happened" meta={source.caption}>
      <BoardPanelSection>
        {entries === null ? (
          <p data-testid="mc-board-event-log-loading" className="mc-board-note">
            Loading…
          </p>
        ) : rows.length === 0 ? (
          <p data-testid="mc-board-event-log-empty" className="mc-board-note">
            No events yet.
          </p>
        ) : (
          <div id={logId} className="mc-board-log">
            {rows.map((row) =>
              row.kind === "event" ? (
                <LogRow
                  key={row.key}
                  event={row.event}
                  expanded={openKeys.includes(row.event.key)}
                  refs={refs}
                  onToggle={toggle}
                />
              ) : row.kind === "day" ? (
                <span key={row.key} data-testid="mc-board-event-day" className="mc-board-log-day">
                  {row.label}
                </span>
              ) : (
                <span key={row.key} data-testid="mc-board-event-hour" className="mc-board-log-hour">
                  {row.label}
                </span>
              ),
            )}
          </div>
        )}
        {source.pageCount > 1 && (
          <nav aria-label="Event pages" data-testid="mc-board-event-log-pager" className="mc-board-log-pager">
            <button
              type="button"
              data-testid="mc-board-event-log-newer"
              className="mc-board-chip"
              aria-label="Newer events"
              aria-controls={logId}
              disabled={index === 0}
              onClick={() => setPageIndex(Math.max(0, index - 1))}
            >
              ‹
            </button>
            {pagerPages(index + 1, lastIndex + 1).map((page, position) =>
              page === "gap" ? (
                <span key={`gap-${position}`} aria-hidden="true" className="mc-board-log-pager-gap">
                  …
                </span>
              ) : (
                <button
                  key={page}
                  type="button"
                  data-testid="mc-board-event-log-page"
                  className="mc-board-chip"
                  aria-controls={logId}
                  aria-current={page === index + 1 ? "page" : undefined}
                  onClick={() => setPageIndex(page - 1)}
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
              disabled={index >= lastIndex}
              onClick={() => setPageIndex(Math.min(lastIndex, index + 1))}
            >
              ›
            </button>
          </nav>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}
