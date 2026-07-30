import { invoke, listen } from "../../lib/bridge";

export type ChatSendStatus = {
  send_id: string;
  tab_id: string;
  state: "pending" | "sent" | "failed";
  error: string | null;
  ledger_id: string;
  sequence: number;
  retained_from: number;
};

export type ChatDelivery = {
  status: ChatSendStatus;
  message_id: string;
  message: string;
};

type DeliveryMessage = Pick<ChatDelivery, "message_id" | "message">;
type Subscriber = {
  handler: (delivery: ChatDelivery) => void;
  onRevoked?: (messageId: string) => void;
  seen: Map<string, string>;
};

const statuses = new Map<string, ChatSendStatus>();
const messages = new Map<string, DeliveryMessage>();
const currentSendByMessage = new Map<string, string>();
const subscribers = new Map<string, Set<Subscriber>>();
let currentLedgerId: string | null = null;
let retainedFrom = 0;

function messageKey(tabId: string, messageId: string): string {
  return `${tabId}:${messageId}`;
}

function revokeDelivery(
  sendId: string,
  status: ChatSendStatus,
  message: DeliveryMessage,
): void {
  statuses.delete(sendId);
  messages.delete(sendId);
  const key = messageKey(status.tab_id, message.message_id);
  if (currentSendByMessage.get(key) !== sendId) return;
  currentSendByMessage.delete(key);
  const tabSubscribers = subscribers.get(status.tab_id);
  if (!tabSubscribers) return;
  for (const subscriber of tabSubscribers) {
    subscriber.seen.delete(sendId);
    subscriber.onRevoked?.(message.message_id);
  }
}

function prepareLedger(status: ChatSendStatus): void {
  if (currentLedgerId !== status.ledger_id) {
    for (const sendId of Array.from(currentSendByMessage.values())) {
      const retainedStatus = statuses.get(sendId);
      const message = messages.get(sendId);
      if (retainedStatus && message) revokeDelivery(sendId, retainedStatus, message);
    }
    currentLedgerId = status.ledger_id;
    retainedFrom = status.retained_from;
    statuses.clear();
    messages.clear();
    currentSendByMessage.clear();
    for (const tabSubscribers of subscribers.values()) {
      for (const subscriber of tabSubscribers) subscriber.seen.clear();
    }
  } else {
    retainedFrom = Math.max(retainedFrom, status.retained_from);
  }

  for (const [sendId, retainedStatus] of statuses) {
    if (retainedStatus.sequence >= retainedFrom) continue;
    const message = messages.get(sendId);
    if (message) {
      revokeDelivery(sendId, retainedStatus, message);
    } else {
      statuses.delete(sendId);
    }
  }
}

function deliveryFor(status: ChatSendStatus): ChatDelivery | null {
  const message = messages.get(status.send_id);
  if (!message) return null;
  if (currentSendByMessage.get(messageKey(status.tab_id, message.message_id)) !== status.send_id) {
    return null;
  }
  return { status, ...message };
}

function notify(delivery: ChatDelivery): void {
  const tabSubscribers = subscribers.get(delivery.status.tab_id);
  if (!tabSubscribers) return;
  const fingerprint = `${delivery.status.state}:${delivery.status.error ?? ""}`;
  for (const subscriber of tabSubscribers) {
    if (subscriber.seen.get(delivery.status.send_id) === fingerprint) continue;
    subscriber.seen.set(delivery.status.send_id, fingerprint);
    subscriber.handler(delivery);
  }
}

function ingestStatus(status: ChatSendStatus): void {
  prepareLedger(status);
  if (status.sequence < retainedFrom) return;
  statuses.set(status.send_id, status);
  const delivery = deliveryFor(status);
  if (delivery) notify(delivery);
}

// Show a failed record only while the current backend ledger vouches for it.
// Restarted or retention-evicted sends must not survive as false failures.
function dropUnvouchedFailures(tabId: string, vouched: ChatSendStatus[]): void {
  const vouchedIds = new Set(vouched.map((status) => status.send_id));
  for (const [sendId, status] of Array.from(statuses)) {
    if (status.tab_id !== tabId || status.state !== "failed") continue;
    if (vouchedIds.has(sendId)) continue;
    const message = messages.get(sendId);
    if (!message) continue;
    revokeDelivery(sendId, status, message);
  }
}

export function trackChatSend(
  status: ChatSendStatus,
  message: DeliveryMessage,
): void {
  prepareLedger(status);
  messages.set(status.send_id, message);
  currentSendByMessage.set(messageKey(status.tab_id, message.message_id), status.send_id);
  ingestStatus(statuses.get(status.send_id) ?? status);
}

export function getChatDeliveries(tabId: string): ChatDelivery[] {
  const deliveries: ChatDelivery[] = [];
  for (const sendId of currentSendByMessage.values()) {
    const status = statuses.get(sendId);
    if (!status || status.tab_id !== tabId) continue;
    const delivery = deliveryFor(status);
    if (delivery) deliveries.push(delivery);
  }
  return deliveries.sort((left, right) => left.status.sequence - right.status.sequence);
}

export async function sendTrackedChatMessage(
  tabId: string,
  messageId: string,
  message: string,
): Promise<ChatSendStatus> {
  const pending = await invoke<ChatSendStatus>("chat_send_text", {
    tabId,
    text: message,
  });
  trackChatSend(pending, { message_id: messageId, message });
  return pending;
}

export async function retryChatDelivery(delivery: ChatDelivery): Promise<ChatSendStatus> {
  return sendTrackedChatMessage(
    delivery.status.tab_id,
    delivery.message_id,
    delivery.message,
  );
}

export function observeChatSendStatus(
  tabId: string,
  onDelivery: (delivery: ChatDelivery) => void,
  onRevoked?: (messageId: string) => void,
): () => void {
  let disposed = false;
  let unlisten: (() => void) | null = null;
  let reconcileRequest = 0;
  const subscriber: Subscriber = { handler: onDelivery, onRevoked, seen: new Map() };
  const tabSubscribers = subscribers.get(tabId) ?? new Set<Subscriber>();
  tabSubscribers.add(subscriber);
  subscribers.set(tabId, tabSubscribers);
  for (const delivery of getChatDeliveries(tabId)) notify(delivery);

  const reconcile = () => {
    const request = ++reconcileRequest;
    void invoke<ChatSendStatus[]>("chat_send_status", { tabId })
      .then((nextStatuses) => {
        if (disposed || request !== reconcileRequest) return;
        for (const status of nextStatuses) ingestStatus(status);
        dropUnvouchedFailures(tabId, nextStatuses);
      })
      .catch(() => {});
  };
  void listen<ChatSendStatus>("chat-send-status", (event) => {
    if (disposed || event.payload.tab_id !== tabId) return;
    ingestStatus(event.payload);
    reconcile();
  }).then(
    (cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      unlisten = cleanup;
      reconcile();
    },
    () => {
      if (!disposed) reconcile();
    },
  );

  return () => {
    disposed = true;
    unlisten?.();
    const activeSubscribers = subscribers.get(tabId);
    activeSubscribers?.delete(subscriber);
    if (activeSubscribers?.size === 0) subscribers.delete(tabId);
  };
}

export function resetChatSendStatusForTest(): void {
  statuses.clear();
  messages.clear();
  currentSendByMessage.clear();
  subscribers.clear();
  currentLedgerId = null;
  retainedFrom = 0;
}

export function chatSendFailureTombstoneCountForTest(): number {
  return Array.from(statuses.values()).filter((status) => status.state === "failed").length;
}
