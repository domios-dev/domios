import { deliveryPresentation, type DeliverySend } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";

export interface QueuedSendKey {
  sendId: string;
  correlationId: string;
  version: number;
}

export type QueuedSendEdit = QueuedSendKey & { appended: string };
export type QueuedSendItem = QueuedSendKey & { preview: string };
export type QueuedSendAction = QueuedSendKey & ({ action: "cancel" } | { action: "append"; text: string });
export type QueuedSendOutcome = { outcome: "appended"; edit: QueuedSendEdit } | { outcome: "cancelled" | "unavailable" };

export interface ComposerSendQueue {
  items: QueuedSendItem[];
  claimed: string[];
  edits: QueuedSendEdit[];
}

export const EMPTY_QUEUE: ComposerSendQueue = { items: [], claimed: [], edits: [] };

export function applyQueuedSendEdit(send: DeliverySend, edit: QueuedSendEdit): DeliverySend {
  if (send.correlationId !== edit.correlationId || (send.status && send.status.send_id !== edit.sendId)
    || edit.version <= (send.queueEdit?.version ?? 0)) return send;
  const prior = send.queueEdit?.appended ?? "";
  if (!edit.appended.startsWith(prior)) return send;
  const addition = edit.appended.slice(prior.length);
  return {
    ...send,
    content: send.content + addition,
    ...(send.draft ? { draft: { ...send.draft, text: send.draft.text + addition } } : {}),
    queueEdit: { version: edit.version, appended: edit.appended },
  };
}

export function composerSendFeedback(
  sends: readonly DeliverySend[], statuses: readonly ChatSendStatus[], queue: ComposerSendQueue,
) {
  const items = queue.items.filter(item => {
    const status = statuses.find(status => status.send_id === item.sendId);
    return !status || status.state === "held";
  });
  const sending = sends.some(send => {
    if (send.dismissed || send.status?.system_send) return false;
    const state = deliveryPresentation(send);
    return state === "sending" || state === "written" || (send.status?.send_id != null && queue.claimed.includes(send.status.send_id) && state === "queued");
  }) || statuses.some(status => !status.system_send && status.correlationId != null
    && (status.state === "written" || (status.state === "held" && queue.claimed.includes(status.send_id))));
  const held = sends.some(send => !send.dismissed && !send.status?.system_send && deliveryPresentation(send) === "queued")
    || statuses.some(status => !status.system_send && status.correlationId != null && status.state === "held");
  return { sending, items, unsettled: sending || held || items.length > 0 };
}
