import type { ChatSend, FrameKind, SendEcho, NormalizedChatMessage } from "@codeterm/chat-core";
import { matchesMatchedRow, normalizeChatText, parseDomiosMessage, timestampMs } from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";
import type { TabActivity } from "../../lib/tabActivity";
import type { CapsuleRegistry } from "./composerCapsules";
import type { ComposerReply } from "./composerReply";

export type DeliveryState = "sending" | "queued" | "uncertain" | "delivered" | "failed";
export interface DeliveryDraft {
  text: string;
  attachments: string[];
  recordedVoiceAttachments: string[];
  capsules: CapsuleRegistry;
  reply: ComposerReply | null;
}
export type DeliverySend = ChatSend & { droppedUploads?: string[]; dismissed?: boolean; status?: ChatSendStatus; requestFailed?: boolean; requestUncertain?: boolean; requestHadResponse?: boolean; draft?: DeliveryDraft };

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

export function deliveryState(send: DeliverySend): DeliveryState {
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

export function deliveryPresentation(send: DeliverySend, activity: TabActivity["kind"]): DeliveryState {
  const state = deliveryState(send);
  if (send.status?.state !== "written" && send.status?.state !== "submit_unconfirmed") return state;
  if (state === "delivered") return state;
  if (activity === "working") return "queued";
  return activity === "idle" ? "uncertain" : "sending";
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
  if (send.status?.ledger_id === status.ledger_id && send.status.sequence === status.sequence
    && send.status.send_id === status.send_id && send.status.state === status.state
    && send.status.pending_reason === status.pending_reason && send.status.error === status.error
    && sameMatchedRow(send.status.matched_row, status.matched_row)) return send;
  return {
    ...send,
    matchedRow: status.matched_row ?? send.matchedRow,
    status,
    requestFailed: false,
    requestUncertain: false,
    echo: status.state === "failed" ? undefined : send.echo,
  };
}

export type AdmissionEvidence = "present" | "absent" | "unknown";

export function reconcileAdmissionEvidence(send: DeliverySend, evidence: AdmissionEvidence): DeliverySend {
  if (send.status || deliveryState(send) === "delivered" || send.requestFailed) return send;
  const failed = evidence === "absent" && send.requestHadResponse === false;
  const uncertain = !failed;
  if (send.requestFailed === failed && send.requestUncertain === uncertain) return send;
  return { ...send, requestFailed: failed, requestUncertain: uncertain };
}

export function reconcileDeliverySnapshot(send: DeliverySend, statuses: readonly ChatSendStatus[]): DeliverySend {
  if (!send.status || deliveryState(send) === "delivered"
    || statuses.some((status) => status.send_id === send.status?.send_id)) return send;
  if (send.status.state === "written") {
    return { ...send, status: { ...send.status, state: "submit_unconfirmed", pending_reason: undefined } };
  }
  return send.status.state === "failed" ? { ...send, status: undefined } : send;
}

export function sendForRow(sends: readonly DeliverySend[], row: NormalizedChatMessage): DeliverySend | undefined {
  if (row.type !== "user" && row.type !== "local_command") return;
  return sends.find((send) => send.id === row.id || (row.correlationId != null && send.correlationId === row.correlationId))
    ?? sends.find((send) => send.matchedRow != null && matchesMatchedRow(row, send.matchedRow))
    ?? sends.find((send) => !send.matchedRow && deliveryState(send) !== "failed" && send.echo?.timestampMs === timestampMs(row.timestamp)
      && send.echo.content === normalizeChatText(row.content).toLowerCase());
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
  sends: readonly DeliverySend[], resetCutoffMs: number | null = null,
): readonly (DeliverySend & { echoEligible: boolean })[] {
  return sends.filter((send) => send.dismissed || resetCutoffMs == null || deliveryState(send) === "failed" || send.sentAtMs >= resetCutoffMs)
    .map((send) => {
      const frame = send.status?.system_send ? parseDomiosMessage(send.content) : null;
      return {
        ...send,
        segments: frame ? [{ type: "frame" as const, kind: frame.attrs.from as FrameKind, from: frame.attrs.from, attrs: frame.attrs, body: frame.body, tip: null }] : send.segments,
        echoEligible: deliveryState(send) !== "failed",
        flowAtMs: send.sentAtMs,
      };
    });
}

export function canDeleteChatSend(send: DeliverySend): boolean {
  return !send.dismissed && !!send.correlationId && !send.status?.system_send
    && deliveryState(send) !== "delivered"
    && (!!send.status || deliveryState(send) === "failed" || deliveryState(send) === "uncertain");
}

export function visibleDeliveryRows(
  rows: readonly NormalizedChatMessage[], sends: readonly DeliverySend[], echoes?: ReadonlyMap<string, SendEcho>,
): NormalizedChatMessage[] {
  const reconciled = echoes ? sends.map((send) => ({ ...send, echo: send.echo ?? echoes.get(send.id) })) : sends;
  return rows.filter((row) => !sendForRow(reconciled, row)?.dismissed);
}

export function canRetryChatSend(send: DeliverySend): boolean {
  if (send.dismissed || send.status?.system_send || deliveryState(send) !== "failed") return false;
  const draft = send.draft;
  return !draft || Boolean(draft.text.trim() || draft.attachments.length || draft.recordedVoiceAttachments.length || draft.reply);
}
