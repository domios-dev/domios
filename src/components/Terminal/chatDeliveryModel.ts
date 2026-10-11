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
export type DeliverySend = ChatSend & { status?: ChatSendStatus; requestFailed?: boolean; draft?: DeliveryDraft };

export function deliveryState(send: DeliverySend): DeliveryState {
  if (send.echo || send.status?.state === "delivered") return "delivered";
  if (send.status?.state === "failed" || (!send.status && send.requestFailed)) return "failed";
  if (send.status?.state === "submit_unconfirmed") return "uncertain";
  return send.status?.state === "held" ? "queued" : "sending";
}

export function applyDeliveryStatus(send: DeliverySend, status: ChatSendStatus): DeliverySend {
  if (status.correlationId !== send.correlationId) return send;
  if (send.status?.ledger_id === status.ledger_id && send.status.sequence > status.sequence) return send;
  if (deliveryState(send) === "delivered" && status.state !== "delivered") return send;
  if (send.status?.ledger_id === status.ledger_id && send.status.sequence === status.sequence
    && send.status.send_id === status.send_id && send.status.state === status.state
    && send.status.pending_reason === status.pending_reason && send.status.error === status.error) return send;
  return { ...send, status, requestFailed: false };
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
    ?? sends.find((send) => send.echo?.timestampMs === timestampMs(row.timestamp)
      && send.echo.content === normalizeChatText(row.content).toLowerCase());
}
