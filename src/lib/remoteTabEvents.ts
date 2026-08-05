import {
  createRemoteChannelRecovery, createSilenceDeadline,
  type RemoteChannelRecovery, type StreamEndReason,
} from "./remoteChannelRecovery";
import {
  classifyTabTarget,
  onRemoteTransportCredentials,
  parseCompositeTabId,
  remoteTabTransportHttp,
} from "./remoteTabTransport";
import { publishRemoteTransportState } from "./remoteTabTransportState";

interface ChatCursor {
  boot: string;
  seq: number;
}

// Forward progress = new boot generation or higher seq; replayed frames dedup.
const chatCursorAdvanced = (prev: ChatCursor | null, next: ChatCursor): boolean =>
  prev === null || next.boot !== prev.boot || next.seq > prev.seq;

interface SubscriptionRecord {
  event: string;
  ordinaryTabId: string;
  peerId: string;
  remoteTabId: string;
  handler: (value: { payload: unknown }) => void;
  controller: AbortController | null;
  recovery: RemoteChannelRecovery<ChatCursor>;
  disposed: boolean;
  settlements: Map<number, { promise: Promise<void>; resolve: () => void }>;
}

const subscriptions = new Map<string, SubscriptionRecord>();

function connect(record: SubscriptionRecord): void {
  if (record.disposed) return;
  record.controller?.abort();
  const controller = new AbortController();
  const generation = record.recovery.beginConnection();
  let settle!: () => void;
  const settlement = new Promise<void>((resolve) => { settle = resolve; });
  record.settlements.set(generation, { promise: settlement, resolve: settle });
  record.controller = controller;
  // Silence deadline (see createSilenceDeadline): re-armed on every byte incl the
  // peer's 15s keep-alive; true silence parks so an ordinary resume can wake it.
  const deadline = createSilenceDeadline(record.recovery, generation, controller);
  deadline.arm();
  const params = new URLSearchParams({
    peer: record.peerId,
    path: "/api/events/stream",
    tab: record.ordinaryTabId,
  });
  const cursor = record.recovery.currentCursor();
  if (cursor) params.set("since", `${cursor.boot}:${cursor.seq}`);
  const { base, token } = remoteTabTransportHttp();
  let reason: StreamEndReason = "eof";
  void fetch(`${base}/api/mesh/relay-stream?${params.toString()}`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    signal: controller.signal,
  }).then(async (response) => {
    if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (!controller.signal.aborted && record.recovery.isCurrent(generation)) {
      const chunk = await reader.read();
      if (chunk.done) break;
      deadline.arm();
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
          let progressed = true;
          if (message.boot && typeof message.seq === "number") {
            progressed = record.recovery.noteProgress(generation, { boot: message.boot, seq: message.seq });
          }
          if (progressed && message.event_type === `tab-chat-msg-${record.remoteTabId}` && message.data) {
            publishRemoteTransportState(record.ordinaryTabId, "chat", "available");
            record.handler({ payload: { ...message.data, tab_id: record.ordinaryTabId } });
          }
        }
        boundary = buffer.indexOf("\n\n");
      }
    }
  }).catch(() => { reason = "error"; }).finally(() => {
    deadline.clear();
    const ownsGeneration = generation === record.recovery.generation();
    if (ownsGeneration) record.controller = null;
    const settled = record.settlements.get(generation);
    record.settlements.delete(generation);
    settled?.resolve();
    if (controller.signal.aborted) return;
    const decision = record.recovery.handleStreamEnd(generation, reason);
    if (decision === "reconnect") connect(record);
    else if (decision === "wait") publishRemoteTransportState(record.ordinaryTabId, "chat", "reconnecting");
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
    recovery: createRemoteChannelRecovery<ChatCursor>({ channel: "chat", advanced: chatCursorAdvanced }),
    disposed: false,
    settlements: new Map(),
  };
  subscriptions.set(event, record);
  connect(record);
  return () => {
    record.disposed = true;
    record.recovery.dispose();
    record.controller?.abort();
    if (subscriptions.get(event) === record) subscriptions.delete(event);
  };
}

onRemoteTransportCredentials(() => resumeRemoteTabEvents());

export function resumeRemoteTabEvents(): void {
  for (const record of subscriptions.values()) {
    // Never supersede a connection still awaiting its first frame (the generation-
    // churn race that blanked remote panes). Reopen once it has a cursor to resume
    // from, or when it is parked awaiting a signal.
    if (record.recovery.currentCursor() == null && !record.recovery.isAwaitingSignal()) continue;
    record.recovery.signal();
    connect(record);
  }
}

export function remoteTabEventSubscriptionSnapshot(event: string): {
  cursor: string | null; generation: number; disposed: boolean; hasController: boolean;
} | null {
  const record = subscriptions.get(event);
  if (!record) return null;
  const cursor = record.recovery.currentCursor();
  return {
    cursor: cursor ? `${cursor.boot}:${cursor.seq}` : null,
    generation: record.recovery.generation(),
    disposed: record.disposed,
    hasController: record.controller !== null,
  };
}

export function waitForRemoteTabEventGenerationSettlement(event: string, generation: number): Promise<void> {
  return subscriptions.get(event)?.settlements.get(generation)?.promise ?? Promise.resolve();
}
