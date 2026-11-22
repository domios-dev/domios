import { baseDaemonTransport } from "./daemonBootstrap";
import { createRemoteChannelRecovery, createSilenceDeadline, streamResponseFailureReason } from "./remoteChannelRecovery";
import type { TransportClient } from "./transportClient";
import type { TabStatus } from "./types";
import type { CommittedPlanChange } from "../../gen/CommittedPlanChange";

export type PeerEvent =
  | { type: "reconcile" }
  | { type: "invalidate" }
  | { type: "tab-status-changed"; data: { tab_id: string; status: TabStatus } }
  | { type: "chat-send-status"; data: { tab_id: string } }
  | { type: "plan-changed"; data: CommittedPlanChange }
  | { type: "plans-loaded" }
  | { type: "mux-state-changed"; data: { generation?: number } };

interface Cursor { boot: string; seq: number }
type Listener = (event: PeerEvent) => void;

function openPeerLink(peerId: string, link: string, publish: Listener, transport: () => TransportClient) {
  const recovery = createRemoteChannelRecovery<Cursor>({
    channel: "control",
    advanced: (previous, next) => !previous || previous.boot !== next.boot || previous.seq < next.seq,
  });
  let controller: AbortController | undefined;
  let closed = false;

  const connect = () => {
    const active = new AbortController();
    controller = active;
    const generation = recovery.beginConnection();
    const deadline = createSilenceDeadline(recovery, generation, active);
    const params = new URLSearchParams({ peer: peerId, path: "/api/events/stream", scope: "peer", link });
    const cursor = recovery.currentCursor();
    if (cursor) params.set("since", `${cursor.boot}:${cursor.seq}`);
    let replaying = true;
    let admitted = false;

    deadline.arm();
    transport().subscribe(`/api/mesh/relay-stream?${params}`, (_name, raw) => {
      if (active.signal.aborted || !recovery.isCurrent(generation) || !raw || typeof raw !== "object") return;
      const frame = raw as { event_type?: string; boot?: string; seq?: number; data?: unknown };
      if (frame.event_type === "peer-replay-complete") {
        if (!replaying) return;
        if (typeof frame.boot === "string" && typeof frame.seq === "number") {
          recovery.noteProgress(generation, { boot: frame.boot, seq: frame.seq });
        }
        replaying = false;
        publish({ type: "reconcile" });
        return;
      }
      if (frame.event_type === "overflow") {
        const reset = frame.data as { boot?: string; next_seq?: number } | null;
        if (typeof reset?.boot === "string" && typeof reset.next_seq === "number") {
          recovery.noteProgress(generation, { boot: reset.boot, seq: reset.next_seq - 1 });
        }
        if (!replaying) publish({ type: "reconcile" });
        return;
      }
      if (typeof frame.boot !== "string" || typeof frame.seq !== "number"
        || !recovery.noteProgress(generation, { boot: frame.boot, seq: frame.seq })) return;
      if (replaying) return;
      if (frame.event_type === "mux-state-changed") {
        const generation = (frame.data as { generation?: unknown } | null)?.generation;
        publish({ type: "mux-state-changed", data: typeof generation === "number" ? { generation } : {} });
      }
      if (frame.event_type === "tab-status-changed") {
        const data = frame.data as { tab_id?: string; status?: TabStatus } | null;
        if (data && typeof data.tab_id === "string" && typeof data.status === "string") {
          publish({ type: "tab-status-changed", data: { tab_id: data.tab_id, status: data.status } });
        }
      }
      if (frame.event_type === "chat-send-status") {
        const data = frame.data as { tab_id?: unknown } | null;
        if (typeof data?.tab_id === "string") {
          publish({ type: "chat-send-status", data: { tab_id: data.tab_id } });
        }
      }
      if (frame.event_type === "plan-changed") {
        const data = frame.data as Partial<CommittedPlanChange> | null;
        if (typeof data?.plan_id === "string" && typeof data.revision === "string"
          && typeof data.previous_revision === "string" && typeof data.reset === "boolean"
          && typeof data.deleted === "boolean" && Array.isArray(data.task_ids)
          && data.task_ids.every(id => typeof id === "string")) {
          publish({ type: "plan-changed", data: data as CommittedPlanChange });
        }
      }
      if (frame.event_type === "plans-loaded") publish({ type: "plans-loaded" });
    }, {
      raw: true,
      signal: active.signal,
      onStarting: deadline.clear,
      onReady: () => { admitted = true; replaying = true; deadline.arm(); },
      onActivity: deadline.arm,
      onEnd(end) {
        deadline.clear();
        if (!recovery.isCurrent(generation) || active.signal.aborted) return;
        const reason = end.reason === "http" ? streamResponseFailureReason(end.status ?? 500)
          : end.reason === "eof" ? "eof" : "error";
        if (admitted || !replaying) publish({ type: "invalidate" });
        if (recovery.handleStreamEnd(generation, reason) === "reconnect") connect();
      },
    });
  };
  return {
    connect,
    resume() { if (!closed && recovery.isAwaitingSignal()) connect(); },
    close() {
      closed = true;
      recovery.dispose();
      controller?.abort();
    },
  };
}

export function createPeerEventRail(transport = baseDaemonTransport) {
  const observers = new Map<string, Set<Listener>>();
  const links = new Map<string, {
    link: string;
    listeners: Set<Listener>;
    stream: ReturnType<typeof openPeerLink>;
  }>();
  return {
    subscribe(peerId: string, link: string, listener: Listener): () => void {
      let current = links.get(peerId);
      if (current?.link !== link) {
        current?.stream.close();
        const listeners = new Set([listener]);
        const stream = openPeerLink(peerId, link, event => {
          for (const receive of listeners) receive(event);
          for (const receive of observers.get(peerId) ?? []) receive(event);
        }, transport);
        current = { link, listeners, stream };
        links.set(peerId, current);
        stream.connect();
      } else current.listeners.add(listener);
      const record = current;
      return () => {
        record.listeners.delete(listener);
        if (!record.listeners.size && links.get(peerId) === record) {
          links.delete(peerId);
          record.stream.close();
        }
      };
    },
    observe(peerId: string, listener: Listener): () => void {
      const listeners = observers.get(peerId) ?? new Set();
      listeners.add(listener);
      observers.set(peerId, listeners);
      return () => {
        listeners.delete(listener);
        if (!listeners.size) observers.delete(peerId);
      };
    },
    resume(peerId: string) { links.get(peerId)?.stream.resume(); },
  };
}

export const peerEventRail = createPeerEventRail();
