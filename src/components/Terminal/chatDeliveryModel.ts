import type { ChatSend, NormalizedChatMessage } from "@codeterm/chat-core";
import { normalizeChatText, timestampMs } from "@codeterm/chat-core";
import type { ChatSendStatus } from "./chatSendStatus";
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
export type DeliverySend = ChatSend & { status?: ChatSendStatus; requestFailed?: boolean; requestUncertain?: boolean; requestHadResponse?: boolean; draft?: DeliveryDraft };

export function deliveryState(send: DeliverySend): DeliveryState {
  if (send.status?.state === "failed" || (!send.status && send.requestFailed)) return "failed";
  if (send.echo || send.status?.state === "delivered") return "delivered";
  if (!send.status && send.requestUncertain) return "uncertain";
  if (send.status?.state === "submit_unconfirmed") return "uncertain";
  return send.status?.state === "held" ? "queued" : "sending";
}

export function applyDeliveryStatus(send: DeliverySend, status: ChatSendStatus): DeliverySend {
  if (status.correlationId !== send.correlationId) return send;
  if (send.status?.ledger_id === status.ledger_id && send.status.sequence > status.sequence) return send;
  if (send.status?.state === "delivered" && status.state !== "delivered") return send;
  if (send.status?.send_id === status.send_id && send.status.ledger_id === status.ledger_id) {
    if (send.status.state === "failed" && status.state !== "failed" && status.state !== "delivered") return send;
    if ((send.status.state === "written" || send.status.state === "submit_unconfirmed") && status.state === "held") return send;
  }
  if (send.status?.ledger_id === status.ledger_id && send.status.sequence === status.sequence
    && send.status.send_id === status.send_id && send.status.state === status.state
    && send.status.pending_reason === status.pending_reason && send.status.error === status.error) return send;
  return { ...send, status, requestFailed: false, requestUncertain: false, echo: status.state === "failed" ? undefined : send.echo };
}

export type AdmissionEvidence = "present" | "absent" | "unknown";

export function reconcileAdmissionEvidence(send: DeliverySend, evidence: AdmissionEvidence): DeliverySend {
  if (send.status || deliveryState(send) === "delivered" || send.requestFailed) return send;
  const failed = evidence === "absent" && send.requestHadResponse === false;
  return { ...send, requestFailed: failed, requestUncertain: !failed };
}

export function reconcileDeliverySnapshot(send: DeliverySend, statuses: readonly ChatSendStatus[]): DeliverySend {
  if (!send.status || deliveryState(send) === "delivered"
    || statuses.some((status) => status.send_id === send.status?.send_id)) return send;
  if (send.status.state === "held") {
    return { ...send, status: { ...send.status, state: "submit_unconfirmed", pending_reason: undefined } };
  }
  return send.status.state === "failed" ? { ...send, status: undefined } : send;
}

export function sendForRow(sends: readonly DeliverySend[], row: NormalizedChatMessage): DeliverySend | undefined {
  if (row.type !== "user" && row.type !== "local_command") return;
  return sends.find((send) => send.id === row.id || (row.correlationId != null && send.correlationId === row.correlationId))
    ?? sends.find((send) => deliveryState(send) !== "failed" && send.echo?.timestampMs === timestampMs(row.timestamp)
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
  return sends.filter((send) => resetCutoffMs == null || deliveryState(send) === "failed" || send.sentAtMs >= resetCutoffMs)
    .map((send) => ({ ...send, echoEligible: deliveryState(send) !== "failed" }));
}
