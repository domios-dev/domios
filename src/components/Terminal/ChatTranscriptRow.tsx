import { memo, useMemo, type ReactNode } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { InteractiveAnswerHandler, OutboundReportDescriptor } from "@codeterm/chat-ui";
import { ChatDaySeparator, ChatHighlightRow, MessageBubble, MessageTimeFrame, OutboundReportCard, messageRowTime } from "@codeterm/chat-ui";
import type { Notification } from "../../lib/types";
import type { ChatOrigin } from "../../lib/chatPrefixes";
import { ChatAskCard } from "../Notifications/ChatAskCard";
import { ChatWindowRow } from "./ChatWindowRow";
import { ChatMessageRow, type MessageRowHandlers } from "./ChatMessageRow";
import { ChatDeliveryState } from "./ChatDeliveryState";
import { ChatLiveOperation } from "./ChatLiveOperation";
import { DroppedUploadNotice } from "./DroppedUploadNotice";
import { VoiceTranscriptionProgress } from "./voice/VoiceTranscriptionProgress";
import { canRetryChatSend, canDeleteChatSend, canEditChatSend, resendsWithCli, type DeliverySend, type DeliveryState } from "./chatDeliveryModel";
import { editChatDelivery } from "./chatDeliveryComposer";
import type { EnrichedChatEntry } from "./chatEnrichment";

type MessageBubbleProps = React.ComponentProps<typeof MessageBubble>;

export interface OutboundReportEntry {
  targetTabId: string;
  at: number | null;
  descriptor: OutboundReportDescriptor;
}

export interface OriginRowProps {
  origin?: ChatOrigin;
  onOriginClick?: () => void;
  originAvatar?: ReactNode;
}

/** Services every transcript row shares; its identity changes only when one of them does. */
export interface ChatTranscriptRowContext {
  tabId: string;
  promptAnswers: boolean;
  uiFontScale?: number;
  answeredInteractiveIds: Set<string>;
  handlers: MessageRowHandlers;
  onContentLayout: MessageBubbleProps["onContentLayout"];
  resolveTabName: MessageBubbleProps["resolveTabName"];
  resolvePeerName: MessageBubbleProps["resolvePeerName"];
  onOpenPlan: MessageBubbleProps["onOpenPlan"];
  resolvePlanRef: MessageBubbleProps["resolvePlanRef"];
  onAnswerInteractive?: InteractiveAnswerHandler;
  onRerunShellCommand: MessageBubbleProps["onRerunShellCommand"];
  onStopReminder: MessageBubbleProps["onStopReminder"];
  onOpenJobs: MessageBubbleProps["onOpenJobs"];
  onJumpToShellCommand?: MessageBubbleProps["onJumpToShellCommand"];
  onFocusTab: (tabRef: string) => void;
  onJumpToMessage: MessageBubbleProps["onJumpToMessage"];
  onAnswerAsk: (id: string, label: string) => void;
  deleteSend: (sendId: string) => void;
  retrySend: (sendId: string) => void;
  forkFrom: (turnId: string) => void;
  revertTo: (turnId: string) => void;
}

export interface ChatTranscriptRowProps {
  entry: EnrichedChatEntry;
  daySeparator?: string;
  originProps?: OriginRowProps;
  reports: readonly OutboundReportEntry[];
  asks: readonly Notification[];
  liveActivityLabel?: string;
  hideMessage?: boolean;
  /** The running turn's live operation, drawn above the send queued behind that turn. */
  leadingLiveOperation?: NormalizedChatMessage | null;
  deliverySend?: DeliverySend;
  deliveryView?: DeliveryState;
  deliveryBehindTurn?: boolean;
  sourceOrdinal?: number;
  selected: boolean;
  joinNext: boolean;
  selecting: boolean;
  cursor: boolean;
  streaming: boolean;
  latestFailedToolCall: boolean;
  forkable: boolean;
  revertible: boolean;
  ariaLabel: string;
  context: ChatTranscriptRowContext;
}

export function OutboundReportRow({ entry, context }: { entry: OutboundReportEntry; context: ChatTranscriptRowContext }) {
  return (
    <div
      data-testid="chat-outbound-report-row"
      data-report-id={entry.descriptor.reportId}
      role="article"
      aria-label={`Report to ${entry.descriptor.recipient}`}
    >
      <MessageTimeFrame timestamp={entry.at == null ? undefined : String(entry.at)} side="left">
        <OutboundReportCard
          report={entry.descriptor}
          onFocusTab={() => context.onFocusTab(entry.targetTabId)}
          fontScale={context.uiFontScale}
          onOpenPlan={context.onOpenPlan}
          resolvePlanRef={context.resolvePlanRef}
        />
      </MessageTimeFrame>
    </div>
  );
}

export function AskRow({ ask, context }: { ask: Notification; context: ChatTranscriptRowContext }) {
  return (
    <MessageTimeFrame timestamp={String(ask.timestamp)} side="left">
      <ChatAskCard
        notification={ask}
        onAnswer={context.onAnswerAsk}
        resolveTabName={context.resolveTabName}
        fontScale={context.uiFontScale}
      />
    </MessageTimeFrame>
  );
}

function sameItems<T>(left: readonly T[], right: readonly T[]): boolean {
  return left === right || (left.length === right.length && left.every((item, index) => item === right[index]));
}

/** Rails are rebuilt for every transcript change, so they compare by their entries rather than by array identity. */
function sameRowProps(left: ChatTranscriptRowProps, right: ChatTranscriptRowProps): boolean {
  for (const key of Object.keys(right) as Array<keyof ChatTranscriptRowProps>) {
    if (key === "reports" || key === "asks") continue;
    if (left[key] !== right[key]) return false;
  }
  return sameItems(left.reports, right.reports) && sameItems(left.asks, right.asks);
}

/** One transcript row; it renders again only when its own message, rails, delivery or selection change. */
export const ChatTranscriptRow = memo(function ChatTranscriptRow({ entry, daySeparator, originProps, reports, asks, liveActivityLabel, hideMessage, leadingLiveOperation,
  deliverySend, deliveryView, deliveryBehindTurn, sourceOrdinal, selected, joinNext, selecting, cursor, streaming, latestFailedToolCall,
  forkable, revertible, ariaLabel, context }: ChatTranscriptRowProps) {
  const { msg, item, origin } = entry;
  const pendingDelivery = deliverySend && deliveryView && deliveryView !== "delivered";
  const onRetry = deliverySend && canRetryChatSend(deliverySend) ? () => context.retrySend(deliverySend.id) : undefined;
  const onRemove = deliverySend && canDeleteChatSend(deliverySend) ? () => context.deleteSend(deliverySend.id) : undefined;
  const onEdit = deliverySend && canEditChatSend(deliverySend) ? () => editChatDelivery(context.tabId, deliverySend) : undefined;
  const resendWithCli = Boolean(deliverySend && resendsWithCli(deliverySend));
  const message = useMemo(() => context.promptAnswers ? msg : { ...msg, interactive: [] }, [msg, context.promptAnswers]);
  const turnId = msg.toolFlowId;
  return (
    <ChatWindowRow message={msg} liveActivityLabel={liveActivityLabel}>
      {leadingLiveOperation && <ChatLiveOperation message={leadingLiveOperation} />}
      <ChatDaySeparator label={daySeparator} />
      {!hideMessage && <ChatHighlightRow id={msg.id}>
        <ChatMessageRow
          msg={msg}
          timestamp={messageRowTime(msg, item, origin)?.timestamp}
          deliveryState={deliveryView}
          sourceOrdinal={sourceOrdinal}
          ariaLabel={ariaLabel}
          onDelete={onRemove}
          onForkFromHere={turnId && forkable ? () => context.forkFrom(turnId) : undefined}
          onRevertToHere={turnId && revertible ? () => context.revertTo(turnId) : undefined}
          selected={selected}
          joinNext={joinNext}
          selecting={selecting}
          cursor={cursor}
          handlers={context.handlers}
        >
          <MessageBubble
            message={message}
            onContentLayout={context.onContentLayout}
            streaming={streaming}
            userProgress={deliveryView === "sending" && Boolean(deliverySend?.draft?.recordedVoiceAttachments.length)
              ? <VoiceTranscriptionProgress /> : undefined}
            userFooter={deliverySend ? <>
              {pendingDelivery && <ChatDeliveryState state={deliveryView} error={deliverySend.status?.error} queuedBehindTurn={deliveryBehindTurn} onRetry={onRetry} onRemove={onRemove} onEdit={onEdit} resendWithCli={resendWithCli} />}
              <DroppedUploadNotice tabId={context.tabId} id={deliverySend.id} names={deliverySend.droppedUploads} />
            </> : undefined}
            noticeStatus={pendingDelivery
              ? <ChatDeliveryState inline state={deliveryView} error={deliverySend.status?.error} queuedBehindTurn={deliveryBehindTurn} onRetry={onRetry} onRemove={onRemove} onEdit={onEdit} resendWithCli={resendWithCli} />
              : undefined}
            item={item.kind === "tool" ? item : undefined}
            isLatestFailedToolCall={latestFailedToolCall}
            origin={originProps?.origin ?? origin}
            resolveTabName={context.resolveTabName}
            resolvePeerName={context.resolvePeerName}
            onOpenPlan={context.onOpenPlan}
            resolvePlanRef={context.resolvePlanRef}
            fontScale={context.uiFontScale}
            onAnswerInteractive={context.promptAnswers ? context.onAnswerInteractive : undefined}
            answeredInteractiveIds={context.answeredInteractiveIds}
            onRerunShellCommand={context.onRerunShellCommand}
            onStopReminder={context.onStopReminder}
            onOpenJobs={context.onOpenJobs}
            onJumpToShellCommand={context.onJumpToShellCommand}
            originAvatar={originProps?.originAvatar}
            onOriginClick={originProps?.onOriginClick}
            onFocusTab={context.onFocusTab}
            onJumpToMessage={context.onJumpToMessage}
          />
        </ChatMessageRow>
      </ChatHighlightRow>}
      {reports.map((report) => <OutboundReportRow key={report.descriptor.reportId} entry={report} context={context} />)}
      {asks.map((ask) => <AskRow key={ask.id} ask={ask} context={context} />)}
    </ChatWindowRow>
  );
}, sameRowProps);
