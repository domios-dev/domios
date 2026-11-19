import { createTransportClient, type TransportClient } from "./transportClient";
import { createRemoteChannelRecovery, createSilenceDeadline, streamResponseFailureReason, type RemoteChannelRecovery, type StreamEndReason, } from "./remoteChannelRecovery";
import { classifyTabTarget, onRemoteTransportCredentials, parseCompositeTabId, remoteTabTransportHttp, } from "./remoteTabTransport";
import { publishRemoteTransportState } from "./remoteTabTransportState";
let eventClient: TransportClient | undefined;
export type RemoteTabEventSignal = "ready" | "resync" | "lost";
const lifecycleListeners = new Map<string, Set<(signal: RemoteTabEventSignal) => void>>();

export function onRemoteTabEventSignal(
    event: string,
    handler: (signal: RemoteTabEventSignal) => void,
): () => void {
    const listeners = lifecycleListeners.get(event) ?? new Set();
    listeners.add(handler);
    lifecycleListeners.set(event, listeners);
    return () => {
        listeners.delete(handler);
        if (!listeners.size)
            lifecycleListeners.delete(event);
    };
}

function signalRemoteTabEvent(event: string, signal: RemoteTabEventSignal): void {
    for (const handler of lifecycleListeners.get(event) ?? [])
        handler(signal);
}

export function bindRemoteTabEventsTransport(client: TransportClient): void {
    eventClient = client;
    for (const record of subscriptions.values())
        connect(record);
}
interface ChatCursor {
    boot: string;
    seq: number;
}
// Forward progress = new boot generation or higher seq; replayed frames dedup.
const chatCursorAdvanced = (prev: ChatCursor | null, next: ChatCursor): boolean => prev === null || next.boot !== prev.boot || next.seq > prev.seq;
interface SubscriptionRecord {
    event: string;
    kind: "chat" | "resize-owner" | "runtime" | "preview" | "provider-switch-status";
    ordinaryTabId: string;
    peerId: string;
    remoteTabId: string;
    handler: (value: {
        payload: unknown;
    }) => void;
    controller: AbortController | null;
    recovery: RemoteChannelRecovery<ChatCursor>;
    disposed: boolean;
    settlements: Map<number, {
        promise: Promise<void>;
        resolve: () => void;
    }>;
}
const subscriptions = new Map<string, SubscriptionRecord>();
const previewHandlers = new Map<string, Set<(value: { payload: unknown }) => void>>();
function connect(record: SubscriptionRecord): void {
    if (record.disposed)
        return;
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
    if (cursor)
        params.set("since", `${cursor.boot}:${cursor.seq}`);
    const { base } = remoteTabTransportHttp();
    const client = eventClient ?? createTransportClient({ origin: base, token: () => remoteTabTransportHttp().token });
    client.subscribe(`/api/mesh/relay-stream?${params}`, (_name, data) => {
        if (controller.signal.aborted || !record.recovery.isCurrent(generation))
            return;
        const message = data as {
            event_type?: string;
            data?: Record<string, unknown>;
            boot?: string;
            seq?: number;
        };
        if (record.kind === "chat" && message.event_type === "overflow") {
            const reset = message.data;
            if (typeof reset?.boot === "string" && typeof reset.next_seq === "number") {
                record.recovery.noteProgress(generation, { boot: reset.boot, seq: reset.next_seq - 1 });
            }
            signalRemoteTabEvent(record.event, "resync");
            return;
        }
        let progressed = true;
        if (message.boot && typeof message.seq === "number") {
            const previous = record.recovery.currentCursor();
            progressed = record.recovery.noteProgress(generation, { boot: message.boot, seq: message.seq });
            if (record.kind === "chat" && previous && previous.boot !== message.boot)
                signalRemoteTabEvent(record.event, "resync");
        }
        if (record.kind === "provider-switch-status" && message.event_type === "overflow")
            signalRemoteTabEvent(record.event, "resync");
        if (progressed && message.data && message.event_type === `preview-index-state-${record.remoteTabId}`) {
            for (const handler of previewHandlers.get(record.ordinaryTabId) ?? []) handler({ payload: message.data });
        }
        if (progressed && message.data && record.kind === "chat"
            && message.event_type === `tab-chat-msg-${record.remoteTabId}`
            && (message.data.tab_id == null || message.data.tab_id === record.remoteTabId)) {
            publishRemoteTransportState(record.ordinaryTabId, "chat", "available");
            record.handler({ payload: { ...message.data, tab_id: record.ordinaryTabId } });
        }
        else if (progressed && message.data && record.kind === "resize-owner"
            && message.event_type === "pty-resize-owner-changed"
            && message.data.tab_id === record.remoteTabId
            && typeof message.data.pty_id === "string") {
            record.handler({ payload: {
                    ...message.data,
                    tab_id: record.ordinaryTabId,
                    pty_id: record.ordinaryTabId,
                } });
        }
        else if (progressed && message.data && record.kind === "runtime"
            && message.event_type === "agent-runtime-state" && message.data.tab_id === record.remoteTabId) {
            record.handler({ payload: { ...message.data, tab_id: record.ordinaryTabId } });
        }
        else if (progressed && message.data && record.kind === "provider-switch-status"
            && message.event_type === "provider-switch-status" && message.data.tab_id === record.remoteTabId) {
            record.handler({ payload: { ...message.data, tab_id: record.ordinaryTabId } });
        }
    }, {
        signal: controller.signal, raw: true, onStarting: () => deadline.clear(), onReady: () => {
            deadline.arm();
            if (record.kind === "provider-switch-status")
                signalRemoteTabEvent(record.event, "ready");
            if (record.kind === "chat" && generation > 1)
                signalRemoteTabEvent(record.event, "resync");
        }, onActivity: () => deadline.arm(),
        onEnd(end) {
            const reason: StreamEndReason = end.reason === "http" ? streamResponseFailureReason(end.status ?? 500)
                : end.reason === "eof" ? "eof" : "error";
            deadline.clear();
            const ownsGeneration = generation === record.recovery.generation();
            if (ownsGeneration)
                record.controller = null;
            const settled = record.settlements.get(generation);
            record.settlements.delete(generation);
            settled?.resolve();
            if (ownsGeneration && !record.disposed && record.kind === "provider-switch-status")
                signalRemoteTabEvent(record.event, "lost");
            if (controller.signal.aborted)
                return;
            const decision = record.recovery.handleStreamEnd(generation, reason);
            if (decision === "reconnect")
                connect(record);
            else if (decision === "wait" && record.kind === "chat") {
                publishRemoteTransportState(record.ordinaryTabId, "chat", "reconnecting");
            }
        },
    });
}
export function subscribeRemoteTabEvent<T>(event: string, handler: (value: {
    payload: T;
}) => void): (() => void) | null {
    const runtimePrefix = "agent-runtime-state:";
    const chatPrefix = "tab-chat-msg-";
    const ownerPrefix = "pty-resize-owner-changed:";
    const previewPrefix = "preview-index-state-";
    const providerSwitchPrefix = "provider-switch-status:";
    const kind = event.startsWith(runtimePrefix) ? "runtime" : event.startsWith(chatPrefix) ? "chat"
        : event.startsWith(previewPrefix) ? "preview" : event.startsWith(ownerPrefix) ? "resize-owner"
            : event.startsWith(providerSwitchPrefix) ? "provider-switch-status" : null;
    if (!kind)
        return null;
    const eventTarget = event.slice(kind === "runtime" ? runtimePrefix.length : kind === "chat" ? chatPrefix.length : kind === "preview" ? previewPrefix.length : kind === "provider-switch-status" ? providerSwitchPrefix.length : ownerPrefix.length);
    if (classifyTabTarget(eventTarget).kind === "invalid-remote") {
        throw new Error(`Invalid remote tab id "${eventTarget}"; local event fallback is forbidden`);
    }
    const identity = parseCompositeTabId(eventTarget);
    if (!identity)
        return null;
    if (kind === "preview") {
        const handlers = previewHandlers.get(identity.ordinaryTabId) ?? new Set();
        const receive = handler as (value: { payload: unknown }) => void;
        handlers.add(receive);
        previewHandlers.set(identity.ordinaryTabId, handlers);
        return () => {
            handlers.delete(receive);
            if (!handlers.size) previewHandlers.delete(identity.ordinaryTabId);
        };
    }
    subscriptions.get(event)?.controller?.abort();
    const record: SubscriptionRecord = {
        event,
        kind,
        ordinaryTabId: identity.ordinaryTabId,
        peerId: identity.peerId,
        remoteTabId: identity.remoteTabId,
        handler: handler as (value: {
            payload: unknown;
        }) => void,
        controller: null,
        recovery: createRemoteChannelRecovery<ChatCursor>({
            channel: kind === "chat" ? "chat" : "control",
            advanced: chatCursorAdvanced,
        }),
        disposed: false,
        settlements: new Map(),
    };
    subscriptions.set(event, record);
    connect(record);
    return () => {
        record.disposed = true;
        record.recovery.dispose();
        record.controller?.abort();
        if (subscriptions.get(event) === record)
            subscriptions.delete(event);
    };
}
onRemoteTransportCredentials(() => resumeRemoteTabEvents());
export function resumeRemoteTabEvents(): void {
    for (const record of subscriptions.values()) {
        // Never supersede a connection still awaiting its first frame (the generation- churn race that blanked remote panes).
        if (record.recovery.currentCursor() == null && !record.recovery.isAwaitingSignal())
            continue;
        record.recovery.signal();
        connect(record);
    }
}
export function remoteTabEventSubscriptionSnapshot(event: string): {
    cursor: string | null;
    generation: number;
    disposed: boolean;
    hasController: boolean;
} | null {
    const record = subscriptions.get(event);
    if (!record)
        return null;
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
