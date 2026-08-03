import { classifyTabTarget, parseCompositeTabId, remoteTabTransportHttp } from "./remoteTabTransport";
import { publishRemoteTransportState } from "./remoteTabTransportState";

interface SubscriptionRecord {
  event: string;
  ordinaryTabId: string;
  peerId: string;
  remoteTabId: string;
  handler: (value: { payload: unknown }) => void;
  controller: AbortController | null;
  cursor: string | null;
  generation: number;
  recoveryAvailable: boolean;
  disposed: boolean;
  settlements: Map<number, { promise: Promise<void>; resolve: () => void }>;
}

const subscriptions = new Map<string, SubscriptionRecord>();

function connect(record: SubscriptionRecord, resetRecovery = false): void {
  if (record.disposed) return;
  if (resetRecovery) record.recoveryAvailable = true;
  record.controller?.abort();
  const controller = new AbortController();
  const generation = ++record.generation;
  let settle!: () => void;
  const settlement = new Promise<void>((resolve) => { settle = resolve; });
  record.settlements.set(generation, { promise: settlement, resolve: settle });
  record.controller = controller;
  const params = new URLSearchParams({
    peer: record.peerId,
    path: "/api/events/stream",
    tab: record.ordinaryTabId,
  });
  if (record.cursor) params.set("since", record.cursor);
  const { base, token } = remoteTabTransportHttp();
  void fetch(`${base}/api/mesh/relay-stream?${params.toString()}`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    signal: controller.signal,
  }).then(async (response) => {
    if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (!controller.signal.aborted && generation === record.generation) {
      const chunk = await reader.read();
      if (chunk.done) break;
      buffer += decoder.decode(chunk.value, { stream: true });
      let boundary = buffer.indexOf("\n\n");
      while (boundary >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = frame.split("\n").find((line) => line.startsWith("data:"));
        if (data) {
          const message = JSON.parse(data.slice(5).trim()) as {
            event_type?: string; data?: Record<string, unknown>; boot?: string; seq?: number;
          };
          if (message.boot && typeof message.seq === "number") record.cursor = `${message.boot}:${message.seq}`;
          if (message.event_type === `tab-chat-msg-${record.remoteTabId}` && message.data) {
            publishRemoteTransportState(record.ordinaryTabId, "chat", "available");
            record.handler({ payload: { ...message.data, tab_id: record.ordinaryTabId } });
          }
        }
        boundary = buffer.indexOf("\n\n");
      }
    }
  }).catch(() => undefined).finally(() => {
    const ownsGeneration = generation === record.generation;
    if (ownsGeneration) record.controller = null;
    const settled = record.settlements.get(generation);
    record.settlements.delete(generation);
    settled?.resolve();
    if (!controller.signal.aborted && ownsGeneration && !record.disposed) {
      if (record.recoveryAvailable) {
        record.recoveryAvailable = false;
        publishRemoteTransportState(record.ordinaryTabId, "chat", "reconnecting");
        connect(record);
      } else {
        publishRemoteTransportState(record.ordinaryTabId, "chat", "unavailable");
      }
    }
  });
}

export function subscribeRemoteTabEvent<T>(
  event: string,
  handler: (value: { payload: T }) => void,
): (() => void) | null {
  const prefix = "tab-chat-msg-";
  if (!event.startsWith(prefix)) return null;
  const eventTarget = event.slice(prefix.length);
  if (classifyTabTarget(eventTarget).kind === "invalid-remote") {
    throw new Error(`Invalid remote tab id "${eventTarget}"; local event fallback is forbidden`);
  }
  const identity = parseCompositeTabId(eventTarget);
  if (!identity) return null;
  subscriptions.get(event)?.controller?.abort();
  const record: SubscriptionRecord = {
    event,
    ordinaryTabId: identity.ordinaryTabId,
    peerId: identity.peerId,
    remoteTabId: identity.remoteTabId,
    handler: handler as (value: { payload: unknown }) => void,
    controller: null,
    cursor: null,
    generation: 0,
    recoveryAvailable: true,
    disposed: false,
    settlements: new Map(),
  };
  subscriptions.set(event, record);
  connect(record);
  return () => {
    record.disposed = true;
    record.controller?.abort();
    if (subscriptions.get(event) === record) subscriptions.delete(event);
  };
}

export function resumeRemoteTabEvents(): void {
  for (const record of subscriptions.values()) connect(record, true);
}

export function remoteTabEventSubscriptionSnapshot(event: string): {
  cursor: string | null; generation: number; disposed: boolean; hasController: boolean;
} | null {
  const record = subscriptions.get(event);
  return record ? {
    cursor: record.cursor,
    generation: record.generation,
    disposed: record.disposed,
    hasController: record.controller !== null,
  } : null;
}

export function waitForRemoteTabEventGenerationSettlement(event: string, generation: number): Promise<void> {
  return subscriptions.get(event)?.settlements.get(generation)?.promise ?? Promise.resolve();
}
