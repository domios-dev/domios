import { createTransportClient, type TransportClient } from "./transportClient";
import { createRemoteChannelRecovery, createSilenceDeadline, streamResponseFailureReason, type RemoteChannelRecovery, type StreamEndReason, } from "./remoteChannelRecovery";
import { classifyTabTarget, onRemoteTransportCredentials, parseCompositeTabId, remoteTabTransportHttp, } from "./remoteTabTransport";
import { publishRemoteTransportState } from "./remoteTabTransportState";
let eventClient: TransportClient | undefined;
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
    kind: "chat" | "resize-owner" | "state";
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
        let progressed = true;
        if (message.boot && typeof message.seq === "number") {
            progressed = record.recovery.noteProgress(generation, { boot: message.boot, seq: message.seq });
        }
        if (progressed && message.data && record.kind === "chat"
            && message.event_type === `tab-chat-msg-${record.remoteTabId}`) {
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
        else if (progressed && record.kind === "state"
            && (message.event_type === "mux-state-changed" || message.event_type === "tab-status-changed")) {
            record.handler({ payload: { event_type: message.event_type, peer_id: record.peerId } });
        }
    }, {
        signal: controller.signal, raw: true, onActivity: () => deadline.arm(),
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
    const chatPrefix = "tab-chat-msg-";
    const ownerPrefix = "pty-resize-owner-changed:";
    const statePrefix = "peer-state-changed:";
    const kind = event.startsWith(chatPrefix) ? "chat"
        : event.startsWith(ownerPrefix) ? "resize-owner"
            : event.startsWith(statePrefix) ? "state"
                : null;
    if (!kind)
        return null;
    const eventTarget = event.slice(kind === "chat" ? chatPrefix.length : kind === "state" ? statePrefix.length : ownerPrefix.length);
    if (classifyTabTarget(eventTarget).kind === "invalid-remote") {
        throw new Error(`Invalid remote tab id "${eventTarget}"; local event fallback is forbidden`);
    }
    const identity = parseCompositeTabId(eventTarget);
    if (!identity)
        return null;
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
onRemoteTransportCredentials(() => resumeRemoteTabEvents(true));
export function resumeRemoteTabEvents(credentialsChanged = false): void {
    for (const record of subscriptions.values()) {
        if (record.kind === "state" && !credentialsChanged && !record.recovery.isAwaitingSignal())
            continue;
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
