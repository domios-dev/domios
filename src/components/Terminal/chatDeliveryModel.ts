import type { ChatSend, FrameKind, SendEcho, NormalizedChatMessage } from "@codeterm/chat-core";
import { matchesMatchedRow, normalizeChatText, outsideClaimedRecordSpace, parseDomiosMessage, timestampMs } from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";
import type { TabActivity } from "../../lib/tabActivity";
import type { CapsuleRegistry } from "./composerCapsules";
import type { ComposerReply } from "./composerReply";
import type { RemoteChatFailure, RemoteChatState } from "../../lib/remoteChatState";
import type { ExpectedChatRecipient } from "../../lib/types";

export type DeliveryState = "sending" | "written" | "queued" | "uncertain" | "delivered" | "failed";
export interface DeliveryDraft {
  text: string;
  attachments: string[];
  recordedVoiceAttachments: string[];
  voiceTranscribing?: boolean;
  capsules: CapsuleRegistry;
  reply: ComposerReply | null;
}
export interface ChatSendTarget {
  expectedRecipient?: ExpectedChatRecipient;
}
export type DeliverySend = ChatSend & { target?: ChatSendTarget; localAttempt?: { ptyId: string; bindingNonce: string }; droppedUploads?: string[]; dismissed?: boolean; status?: ChatSendStatus; requestFailed?: boolean; requestError?: string; requestUncertain?: boolean; requestHadResponse?: boolean; admissionSettled?: boolean; draft?: DeliveryDraft;
  wasHeld?: boolean; remoteFailure?: RemoteChatFailure & { state: RemoteChatState } };

export function chatSendFailureReason(reason?: unknown): string {
  const message = typeof reason === "string" ? reason
    : reason && typeof reason === "object" && "message" in reason && typeof reason.message === "string" ? reason.message : "";
  return message.trim().slice(0, 4096) || "Message was not sent; the failure reason is unavailable.";
}

export function deliveryError(send: DeliverySend): string | null | undefined {
  const reason = send.status?.error || send.requestError;
  return deliveryState(send) === "failed" ? chatSendFailureReason(reason) : reason;
}

function sameMatchedRow(left: ChatSendStatus["matched_row"], right: ChatSendStatus["matched_row"]): boolean {
  if (left == null || right == null) return left === right;
  if (left.ordinal !== right.ordinal || left.source.source_session_id !== right.source.source_session_id) return false;
  const a = left.source.record;
  const b = right.source.record;
  if (a.kind !== b.kind) return false;
  return a.kind === "structured" && b.kind === "structured"
    ? a.thread_id === b.thread_id && a.item_id === b.item_id
    : a.kind === "jsonl" && b.kind === "jsonl"
      && a.incarnation === b.incarnation && a.record_start === b.record_start && a.record_row === b.record_row;
}

function sameSystemSend(left: ChatSendStatus["system_send"], right: ChatSendStatus["system_send"]): boolean {
  if (left == null || right == null) return left === right;
  if (left.content !== right.content || left.sent_at_ms !== right.sent_at_ms
    || (left.segments_version ?? undefined) !== (right.segments_version ?? undefined)) return false;
  if (left.segments === right.segments) return true;
  if (!left.segments || !right.segments || left.segments.length !== right.segments.length) return false;
  return JSON.stringify(left.segments) === JSON.stringify(right.segments);
}

function mergeSystemSend(
  prior: ChatSendStatus["system_send"],
  incoming: ChatSendStatus["system_send"],
  sameSend: boolean,
): ChatSendStatus["system_send"] {
  if (!incoming) return sameSend ? prior : undefined;
  if (!sameSend || !prior || prior.content !== incoming.content) return incoming;
  const incomingSegments = incoming.segments?.length ? incoming.segments : undefined;
  const priorSegments = prior.segments?.length ? prior.segments : undefined;
  if (incomingSegments) {
    if (!priorSegments || (incoming.segments_version ?? 0) >= (prior.segments_version ?? 0)) return incoming;
    return { ...incoming, segments: prior.segments, segments_version: prior.segments_version };
  }
  return priorSegments ? { ...incoming, segments: prior.segments, segments_version: prior.segments_version } : incoming;
}

export function deliveryState(send: DeliverySend): DeliveryState {
  if (!send.correlationId && !send.status && !send.echo) return "uncertain";
  if (send.status?.state === "failed" || (!send.status && send.requestFailed)) return "failed";
  if (send.echo || send.status?.state === "delivered") return "delivered";
  if (!send.status && send.requestUncertain) return "uncertain";
  if (send.status?.state === "submit_unconfirmed") return "uncertain";
  return send.status?.state === "held" ? "queued" : "sending";
}

/** Echoes are recorded from the rendered projection, so a hidden chat stays live until its sends settle. */
export function awaitsTranscriptEcho(sends: readonly DeliverySend[]): boolean {
  return sends.some((send) => {
    const state = deliveryState(send);
    return !send.dismissed && state !== "delivered" && state !== "failed";
  });
}

/** Written input lands when the running turn ends, so missing evidence is uncertain only once that turn is over. */
function writtenAwaitingTurn(send: DeliverySend, activity: TabActivity["kind"]): boolean {
  const status = send.status;
  return activity === "working" && !send.echo && !status?.error
    && (status?.state === "written" || status?.state === "submit_unconfirmed");
}

export function deliveryPresentation(send: DeliverySend, activity: TabActivity["kind"]): DeliveryState {
  if (writtenAwaitingTurn(send, activity)) return "queued";
  return deliveryState(send) === "sending" && send.status?.state === "written" ? "written" : deliveryState(send);
}

export const ACTIVE_TURN_HOLD = "held_active_turn";

/** A send waits for the running turn when the host holds it for that turn, or while the agent is mid-turn. */
export function queuedBehindTurn(send: DeliverySend, activity: TabActivity["kind"]): boolean {
  if (writtenAwaitingTurn(send, activity)) return true;
  return deliveryState(send) === "queued" && activity !== "failed"
    && (send.status?.pending_reason === ACTIVE_TURN_HOLD || activity === "working");
}

/** Sends nothing has confirmed yet: each keeps its bubble until it is delivered or dismissed. */
export function undeliveredSends(sends: readonly DeliverySend[]): readonly DeliverySend[] {
  return sends.filter((send) => !send.dismissed && deliveryState(send) !== "delivered");
}

/** Held sends with no local record, such as those from another window; a status carries no text to draw a bubble from. */
export function unownedHeldSends(sends: readonly DeliverySend[], statuses: readonly ChatSendStatus[]): number {
  return statuses.filter((status) => status.state === "held" && !status.system_send && status.correlationId != null
    && !sends.some((send) => send.correlationId === status.correlationId)).length;
}

export function applyDeliveryStatus(send: DeliverySend, status: ChatSendStatus): DeliverySend {
  if (status.correlationId !== send.correlationId) return send;
  if (send.status?.ledger_id === status.ledger_id && send.status.sequence > status.sequence) return send;
  if (send.status?.state === "delivered" && status.state !== "delivered") return send;
  if (send.status?.send_id === status.send_id && send.status.ledger_id === status.ledger_id) {
    if (send.status.state === "failed" && status.state !== "failed" && status.state !== "delivered") return send;
    if ((send.status.state === "written" || send.status.state === "submit_unconfirmed") && status.state === "held") return send;
    if (send.status.state === "submit_unconfirmed" && status.state === "written") return send;
  }
  const mergedSystemSend = mergeSystemSend(send.status?.system_send, status.system_send, send.status?.send_id === status.send_id);
  const nextStatus = mergedSystemSend === status.system_send ? status : { ...status, system_send: mergedSystemSend };
  if (send.status?.ledger_id === nextStatus.ledger_id && send.status.sequence === nextStatus.sequence
    && send.status.send_id === nextStatus.send_id && send.status.state === nextStatus.state
    && send.status.pending_reason === nextStatus.pending_reason && send.status.error === nextStatus.error
    && sameMatchedRow(send.status.matched_row, nextStatus.matched_row)
    && sameSystemSend(send.status.system_send, nextStatus.system_send)) return send;
  return {
    ...send,
    ...(send.wasHeld || nextStatus.state === "held" ? { wasHeld: true } : {}),
    matchedRow: nextStatus.matched_row ?? send.matchedRow,
    status: nextStatus,
    requestFailed: false,
    ...(send.requestError !== undefined || nextStatus.state === "failed"
      ? { requestError: nextStatus.state === "failed" ? chatSendFailureReason(nextStatus.error || send.requestError) : undefined } : {}),
    requestUncertain: false,
    remoteFailure: undefined,
    echo: nextStatus.state === "failed" ? undefined : send.echo,
  };
}

export type AdmissionEvidence = "present" | "absent" | "unknown" | "not_applicable";

export function reconcileAdmissionEvidence(send: DeliverySend, evidence: AdmissionEvidence): DeliverySend {
  if (send.status || deliveryState(send) === "delivered" || send.requestFailed) return send;
  if (evidence === "not_applicable") return { ...send, admissionSettled: true, requestUncertain: true };
  const failed = evidence === "absent" && send.requestHadResponse === false;
  const uncertain = !failed;
  if (send.requestFailed === failed && send.requestUncertain === uncertain) return send;
  return { ...send, requestFailed: failed, requestUncertain: uncertain,
    ...(failed && !send.requestError ? { requestError: "No admission was recorded for the failed send request; no further failure reason is available." } : {}) };
}

export function reconcileDeliverySnapshot(send: DeliverySend, statuses: readonly ChatSendStatus[]): DeliverySend {
  if (!send.status || deliveryState(send) === "delivered"
    || statuses.some((status) => status.send_id === send.status?.send_id)) return send;
  if (send.status.state === "written") {
    return { ...send, status: { ...send.status, state: "submit_unconfirmed", pending_reason: undefined } };
  }
  return send.status.state === "failed" ? { ...send, status: undefined } : send;
}

function ownsRowToken(send: DeliverySend, row: NormalizedChatMessage): boolean {
  return row.correlationId != null && send.correlationId === row.correlationId
    && (!send.matchedRow || matchesMatchedRow(row, send.matchedRow));
}

function claimsRowRecord(send: DeliverySend, row: NormalizedChatMessage): boolean {
  return send.matchedRow != null && matchesMatchedRow(row, send.matchedRow);
}

function ownsBubble(send: DeliverySend, row: NormalizedChatMessage): boolean {
  return row.id === send.id || (row.sourceMessageId === send.id && row.correlationId != null
    && row.correlationId === send.correlationId && row.inheritedFromSessionId == null);
}

/** A claim links by echo only to its session's rows whose record identity cannot contradict it. */
function echoesRow(send: DeliverySend, row: NormalizedChatMessage): boolean {
  return (!send.matchedRow || outsideClaimedRecordSpace(row, send.matchedRow)) && deliveryState(send) !== "failed"
    && send.echo?.timestampMs === timestampMs(row.timestamp) && send.echo.content === normalizeChatText(row.content).toLowerCase();
}

/** Every send a row settles: one merged record can carry several, while a local bubble belongs to its send alone. */
export function sendsForRow(sends: readonly DeliverySend[], row: NormalizedChatMessage): DeliverySend[] {
  if (row.type !== "user" && row.type !== "local_command") return [];
  const own = sends.find(send => ownsBubble(send, row));
  if (own) return [own];
  return sends.filter(send => ownsRowToken(send, row) || claimsRowRecord(send, row) || echoesRow(send, row));
}

/** The send whose identity a row carries; a merged record prefers a send the owner kept. */
export function sendForRow(sends: readonly DeliverySend[], row: NormalizedChatMessage): DeliverySend | undefined {
  if (row.type !== "user" && row.type !== "local_command") return;
  const own = sends.find(send => ownsBubble(send, row));
  if (own) return own;
  const owners = [...sends.filter(send => ownsRowToken(send, row)), ...sends.filter(send => claimsRowRecord(send, row))];
  return owners.find(send => !send.dismissed) ?? owners[0] ?? sends.find(send => echoesRow(send, row));
}

export function reconcileDeliveryStatusesModel(
  current: readonly DeliverySend[], statuses: readonly ChatSendStatus[],
  requested: readonly DeliverySend[] = current,
): readonly DeliverySend[] {
  return current.map((send) => {
    const prior = requested.find((item) => item.id === send.id);
    const status = statuses.find((item) => item.correlationId != null && item.correlationId === send.correlationId);
    if (status) return applyDeliveryStatus(send, status);
    if (!prior || prior.status !== send.status || prior.correlationId !== send.correlationId
      || prior.requestFailed !== send.requestFailed) return send;
    return reconcileDeliverySnapshot(send, statuses);
  });
}

export function deliveryProjectionSends(
  sends: readonly DeliverySend[], resetCutoffMs: number | null = null, queuedIds: ReadonlySet<string> = new Set(),
): readonly (DeliverySend & { echoEligible: boolean })[] {
  return sends.filter((send) => send.dismissed || resetCutoffMs == null || deliveryState(send) !== "delivered" || send.sentAtMs >= resetCutoffMs)
    .map((send) => {
      const frame = send.status?.system_send ? parseDomiosMessage(send.content) : null;
      return {
        ...send,
        segments: send.status?.system_send?.segments?.length ? send.status.system_send.segments
          : frame ? [{ type: "frame" as const, kind: frame.attrs.from as FrameKind, from: frame.attrs.from, attrs: frame.attrs, body: frame.body,
            tip: frame.content.slice(frame.body.length).match(/<tip>([\s\S]*?)<\/tip>/)?.[1] ?? null }] : send.segments,
        echoEligible: deliveryState(send) !== "failed",
        neverWritten: deliveryState(send) === "queued",
        flowAtMs: send.sentAtMs,
        held: Boolean(send.draft?.recordedVoiceAttachments.length && !send.echo)
          || queuedIds.has(send.id)
          || (deliveryState(send) !== "delivered" && send.status?.pending_reason === ACTIVE_TURN_HOLD),
      };
    });
}

export function canDeleteChatSend(send: DeliverySend): boolean {
  if (send.dismissed || deliveryState(send) === "delivered") return false;
  if (!send.correlationId || send.status?.system_send) return canDismissSettledChatSend(send);
  return !!send.status || deliveryState(send) === "failed" || deliveryState(send) === "uncertain";
}

export function canDismissSettledChatSend(send: DeliverySend): boolean {
  return !send.dismissed && (deliveryState(send) === "failed" || send.status?.state === "submit_unconfirmed"
    || (!send.correlationId && !send.status && deliveryState(send) === "uncertain"));
}

export function canEditChatSend(send: DeliverySend): boolean {
  return !send.dismissed && !send.status?.system_send && deliveryState(send) === "uncertain";
}

export function visibleDeliveryRows(
  rows: readonly NormalizedChatMessage[], sends: readonly DeliverySend[], echoes?: ReadonlyMap<string, SendEcho>,
): NormalizedChatMessage[] {
  const reconciled = echoes ? sends.map((send) => ({ ...send, echo: send.echo ?? echoes.get(send.id) })) : sends;
  return rows.flatMap((row) => {
    const settled = sendsForRow(reconciled, row);
    if (!settled.some(send => send.dismissed)) return [row];
    return settled.every(send => send.dismissed) ? [] : [withoutDismissedSegments(row, settled)];
  });
}

/** A merged record keeps its live sends' text; it drops only segments that carry a dismissed send's token. */
function withoutDismissedSegments(row: NormalizedChatMessage, settled: readonly DeliverySend[]): NormalizedChatMessage {
  const dismissed = new Set(settled.flatMap(send => send.dismissed && send.correlationId ? [send.correlationId] : []));
  const segments = row.segments?.filter(segment => !(segment.correlationId != null && dismissed.has(segment.correlationId)));
  if (!segments || segments.length === row.segments!.length || !segments.some(segment => segment.correlationId != null)) return row;
  const key = [...dismissed].sort().join("\n");
  const cached = strippedRows.get(row);
  if (cached?.key === key) return cached.row;
  const content = segments.every(segment => segment.type === "text")
    ? segments.map(segment => segment.type === "text" ? segment.text : "").join("").trim() : row.content;
  const stripped = { ...row, content, segments };
  strippedRows.set(row, { key, row: stripped });
  return stripped;
}

const strippedRows = new WeakMap<NormalizedChatMessage, { key: string; row: NormalizedChatMessage }>();

/** A routed send carries its sender's frame, which the composer cannot reissue, so the owner resends it with the CLI. */
export function resendsWithCli(send: DeliverySend): boolean {
  return !send.dismissed && Boolean(send.status?.system_send) && deliveryState(send) === "failed";
}

export function canRetryChatSend(send: DeliverySend): boolean {
  if (send.dismissed || send.status?.system_send || deliveryState(send) !== "failed") return false;
  const draft = send.draft;
  return !draft || Boolean(draft.text.trim() || draft.attachments.length || draft.recordedVoiceAttachments.length || draft.reply);
}
