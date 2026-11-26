import { memo } from "react";
import type { Notification } from "../../lib/types";
import type { TabActivity } from "../../lib/tabActivity";
import type { ChatAnchorIdentity } from "./chatAnchorModel";
import type { EnrichedChatEntry } from "./chatEnrichment";
import { deliveryPresentation, deliverySendForRow, queuedBehindTurn, type DeliverySend } from "./chatDeliveryModel";
import { ChatTranscriptRow, type ChatTranscriptRowContext, type OriginRowProps, type OutboundReportEntry } from "./ChatTranscriptRow";
import { HoldWhileHidden } from "./HoldWhileHidden";

const NO_RAIL_INDICES: readonly number[] = [];
const NO_REPORTS: readonly OutboundReportEntry[] = [];
const NO_ASKS: readonly Notification[] = [];

/** A row's own report cards first, then live receipts placed after it. */
function rowReports(
  own: readonly OutboundReportEntry[] | undefined,
  following: readonly number[],
  rail: readonly OutboundReportEntry[],
): readonly OutboundReportEntry[] {
  if (!following.length) return own ?? NO_REPORTS;
  return [...(own ?? []), ...following.map((index) => rail[index]).filter(Boolean)];
}

export interface MountedChatRowsProps {
  visible: boolean;
  rows: readonly EnrichedChatEntry[];
  start: number;
  end: number;
  identities: readonly ChatAnchorIdentity[];
  liveToolId: string | null;
  liveActivityLabel?: string;
  daySeparators: ReadonlyMap<number, string>;
  originPropsById: ReadonlyMap<string, OriginRowProps>;
  reportRowsByRow: ReadonlyMap<number, readonly OutboundReportEntry[]>;
  reportAfterIndex: ReadonlyMap<number, readonly number[]>;
  outboundReports: readonly OutboundReportEntry[];
  askAfterIndex: ReadonlyMap<number, readonly number[]>;
  asks: readonly Notification[];
  deliverySends: readonly DeliverySend[];
  claimedSendIds: readonly string[];
  agentActivityKind: TabActivity["kind"];
  agentActive: boolean;
  selected: ReadonlySet<string>;
  selecting: boolean;
  cursor: string | null;
  streamingId: string | null;
  latestFailedToolCallId: string | null;
  latestStopNoticeId: string | null;
  readOnly: boolean;
  forkableMessages: ReadonlySet<string>;
  canRevertTurns: boolean;
  rowContext: ChatTranscriptRowContext;
}

/** Memoized mounted-window slice: range shifts re-render only entering/leaving rows because each row keeps its key and stable props. */
export const MountedChatRows = memo(function MountedChatRows({
  visible,
  rows,
  start,
  end,
  identities,
  liveToolId,
  liveActivityLabel,
  daySeparators,
  originPropsById,
  reportRowsByRow,
  reportAfterIndex,
  outboundReports,
  askAfterIndex,
  asks,
  deliverySends,
  claimedSendIds,
  agentActivityKind,
  agentActive,
  selected,
  selecting,
  cursor,
  streamingId,
  latestFailedToolCallId,
  latestStopNoticeId,
  readOnly,
  forkableMessages,
  canRevertTurns,
  rowContext,
}: MountedChatRowsProps) {
  return (
    <HoldWhileHidden visible={visible}>
      {rows.slice(start, end).map((entry, mountedIndex) => {
        const { msg } = entry;
        const messageIndex = start + mountedIndex;
        const following = reportAfterIndex.get(messageIndex) ?? NO_RAIL_INDICES;
        const followingAsks = askAfterIndex.get(messageIndex) ?? NO_RAIL_INDICES;
        const nextId = rows[messageIndex + 1]?.msg.id;
        const reports = rowReports(reportRowsByRow.get(messageIndex), following, outboundReports);
        const deliverySend = deliverySendForRow(deliverySends, msg);
        return (
          <ChatTranscriptRow
            key={msg.id}
            entry={entry}
            liveActivityLabel={msg.id === liveToolId ? liveActivityLabel : undefined}
            daySeparator={daySeparators.get(messageIndex)}
            originProps={originPropsById.get(msg.id)}
            reports={reports}
            asks={followingAsks.length ? followingAsks.map((index) => asks[index]).filter(Boolean) : NO_ASKS}
            deliverySend={deliverySend}
            deliveryView={deliverySend ? deliveryPresentation(deliverySend,
              Boolean(deliverySend.status && claimedSendIds.includes(deliverySend.status.send_id))) : undefined}
            deliveryBehindTurn={deliverySend ? queuedBehindTurn(deliverySend, agentActivityKind) : undefined}
            sourceOrdinal={identities[messageIndex].ordinal}
            selected={selected.has(msg.id)}
            joinNext={nextId !== undefined && reports.length === 0 && selected.has(nextId)}
            selecting={selecting}
            cursor={cursor ? cursor === msg.id : messageIndex === rows.length - 1}
            streaming={msg.id === streamingId}
            latestFailedToolCall={msg.id === latestFailedToolCallId}
            canContinueTurn={!readOnly && !agentActive && msg.id === latestStopNoticeId}
            forkable={forkableMessages.has(msg.id)}
            revertible={canRevertTurns}
            context={rowContext}
          />
        );
      })}
    </HoldWhileHidden>
  );
});
