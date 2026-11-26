import type { Notification } from "../../lib/types";
import type { StreamEnd } from "../../lib/transportClient";
import type { AnswerUpdate } from "../../lib/notificationAnswer";
import { createRemoteChannelRecovery, streamResponseFailureReason, type RemoteChannelRecovery, type SilenceDeadlineScheduler, type StreamEndReason } from "../../lib/remoteChannelRecovery";
import { createPeerStreamBackoff, type PeerStreamBackoff, type PeerStreamBackoffOptions } from "../../lib/peerStreamBackoff";
import { remoteNotificationFromPayload, remoteNotificationId } from "../../lib/remoteNotifications";
import { pairedAvailability } from "./pairedWorkspaceProjection";
import type { PairedRoster } from "./useMeshPeerProjection";

export const MAX_BUFFERED_REMOTE_NOTIFICATION_ACTIONS = 512;

export type ConfirmedRemoteNotificationAction =
  | { type: "MARK_READ"; id: string }
  | { type: "DELETE_NOTIFICATION"; id: string }
  | { type: "UPDATE_NOTIFICATION_ANSWER"; id: string; update: Exclude<AnswerUpdate, { pending: true }> };

export interface RemoteNotificationRouteLease {
  isCurrent(): boolean;
  confirm(action: ConfirmedRemoteNotificationAction): void;
}

export type RemoteNotificationSourceAction =
  | ConfirmedRemoteNotificationAction
  | { type: "REPLACE_REMOTE_NOTIFICATION_HISTORY"; peerId: string; notifications: Notification[] }
  | { type: "ADD_NOTIFICATION"; notification: Notification }
  | { type: "UPDATE_NOTIFICATION_SUMMARY"; id: string; title: string; body: string }
  | { type: "RESOLVE_NOTIFICATION"; id: string }
  | { type: "DISMISS_TOAST"; id: string };

export interface RemoteNotificationCursor {
  boot: string;
  seq: number;
}

export interface RemoteNotificationEventHandlers {
  frame(value: unknown): void;
  ready(): void;
  end(value: StreamEnd): void;
}

export interface RemoteNotificationSourcePorts {
  load(peerId: string, signal: AbortSignal): Promise<Notification[]>;
  subscribe(peerId: string, cursor: RemoteNotificationCursor | null, handlers: RemoteNotificationEventHandlers): () => void;
  arrival?(notification: Notification): void;
  backoff?: PeerStreamBackoffOptions;
  scheduler?: SilenceDeadlineScheduler;
}

interface PeerSource {
  peerId: string;
  peerName: string;
  publicKey: string;
  recovery: RemoteChannelRecovery<RemoteNotificationCursor>;
  backoff: PeerStreamBackoff;
  retryTimer: unknown | null;
  generation: number;
  stop?: () => void;
  loadController?: AbortController;
  loading: boolean;
  bufferOverflowed: boolean;
  refreshAgain: boolean;
  buffered: RemoteNotificationSourceAction[];
}

interface RemoteEventFrame {
  event_type: string;
  data?: unknown;
  boot?: string;
  seq?: number;
  prev_seq?: number | null;
}

type Dispatch = (action: RemoteNotificationSourceAction) => void;

export function createRemoteNotificationSource(dispatch: Dispatch, ports: RemoteNotificationSourcePorts) {
  const sources = new Map<string, PeerSource>();
  let disposed = false;

  const isCurrent = (source: PeerSource) => !disposed && sources.get(source.peerId) === source;

  const dispatchBuffered = (source: PeerSource) => {
    const buffered = source.buffered;
    source.buffered = [];
    for (const action of buffered) {
      if (!isCurrent(source)) return;
      dispatch(action);
    }
  };

  const refreshHistory = (source: PeerSource) => {
    if (!isCurrent(source)) return;
    if (source.loading) {
      source.refreshAgain = true;
      return;
    }
    const controller = new AbortController();
    source.loadController = controller;
    source.loading = true;
    source.bufferOverflowed = false;
    source.buffered = [];
    void ports.load(source.peerId, controller.signal).then(rows => {
      if (!isCurrent(source) || controller.signal.aborted) return;
      if (source.bufferOverflowed) {
        source.refreshAgain = true;
        return;
      }
      const notifications = rows.flatMap(row => {
        const projected = remoteNotificationFromPayload(source.peerId, source.peerName, row);
        return projected ? [projected] : [];
      });
      dispatch({ type: "REPLACE_REMOTE_NOTIFICATION_HISTORY", peerId: source.peerId, notifications });
      dispatchBuffered(source);
    }).catch(() => {
      if (!isCurrent(source) || controller.signal.aborted) return;
      if (source.bufferOverflowed) source.refreshAgain = true;
      else dispatchBuffered(source);
    }).finally(() => {
      if (!isCurrent(source)) return;
      source.loadController = undefined;
      source.loading = false;
      if (source.refreshAgain) {
        source.refreshAgain = false;
        refreshHistory(source);
      }
    });
  };

  const sourceAction = (source: PeerSource, frame: RemoteEventFrame): RemoteNotificationSourceAction | null => {
    const data = record(frame.data);
    const sourceId = data && typeof data.id === "string" ? data.id : null;
    if (!data || !sourceId) return null;
    let id: string;
    try {
      id = remoteNotificationId(source.peerId, sourceId);
    } catch {
      return null;
    }
    switch (frame.event_type) {
      case "notification-created": {
        const notification = remoteNotificationFromPayload(source.peerId, source.peerName, data);
        return notification ? { type: "ADD_NOTIFICATION", notification } : null;
      }
      case "notification-updated":
        return typeof data.title === "string"
          ? { type: "UPDATE_NOTIFICATION_SUMMARY", id, title: data.title, body: typeof data.body === "string" ? data.body : "" }
          : null;
      case "notification-answered": {
        if (typeof data.answered !== "string") return null;
        const refusal = record(data.refusal);
        return {
          type: "UPDATE_NOTIFICATION_ANSWER",
          id,
          update: data.submit_unconfirmed === true ? { unconfirmed: true, answered: data.answered } : {
            answered: data.answered,
            delivered: data.delivered === true,
            error: typeof data.deliveryError === "string" ? data.deliveryError
              : typeof refusal?.detail === "string" ? refusal.detail
                : typeof refusal?.reason === "string" ? refusal.reason : undefined,
          },
        };
      }
      case "notification-resolved": return { type: "RESOLVE_NOTIFICATION", id };
      case "notification-cleared": return { type: "DELETE_NOTIFICATION", id };
      default: return null;
    }
  };

  const applyActions = (source: PeerSource, actions: RemoteNotificationSourceAction[]) => {
    if (!source.loading) {
      for (const action of actions) dispatch(action);
      return;
    }
    if (source.bufferOverflowed) return;
    if (source.buffered.length + actions.length > MAX_BUFFERED_REMOTE_NOTIFICATION_ACTIONS) {
      source.buffered = [];
      source.bufferOverflowed = true;
      source.refreshAgain = true;
      return;
    }
    source.buffered.push(...actions);
  };

  const receiveFrame = (source: PeerSource, generation: number, value: unknown) => {
    if (!isCurrent(source) || !source.recovery.isCurrent(generation)) return;
    const frame = record(value) as RemoteEventFrame | null;
    if (!frame || typeof frame.event_type !== "string") return;
    let progressed = true;
    if (typeof frame.boot === "string" && Number.isSafeInteger(frame.seq)) {
      const previous = source.recovery.currentCursor();
      if (previous && previous.boot === frame.boot && frame.prev_seq != null && frame.prev_seq !== previous.seq) {
        refreshHistory(source);
      }
      progressed = source.recovery.noteProgress(generation, { boot: frame.boot, seq: frame.seq! });
    }
    if (frame.event_type === "overflow") {
      refreshHistory(source);
      return;
    }
    if (!progressed) return;
    const action = sourceAction(source, frame);
    if (!action) return;
    const arrival = frame.event_type === "notification-created" && action.type === "ADD_NOTIFICATION"
      ? action.notification
      : undefined;
    if (arrival) ports.arrival?.(arrival);
    const actions = frame.event_type === "notification-cleared" && action.type === "DELETE_NOTIFICATION"
      ? [action, { type: "DISMISS_TOAST", id: action.id } as const]
      : [action];
    applyActions(source, actions);
  };

  const scheduler: SilenceDeadlineScheduler = ports.scheduler ?? {
    schedule(callback, timeoutMs) {
      const timer = setTimeout(callback, timeoutMs);
      (timer as unknown as { unref?: () => void }).unref?.();
      return timer;
    },
    cancel(handle) {
      clearTimeout(handle as ReturnType<typeof setTimeout>);
    },
  };

  const clearRetryTimer = (source: PeerSource) => {
    if (source.retryTimer === null) return;
    scheduler.cancel(source.retryTimer);
    source.retryTimer = null;
  };

  const scheduleRetry = (source: PeerSource, delayMs: number) => {
    if (!isCurrent(source) || source.retryTimer !== null) return;
    source.retryTimer = scheduler.schedule(() => {
      source.retryTimer = null;
      if (!isCurrent(source) || source.stop || source.backoff.isBlocked()) return;
      if (source.recovery.isAwaitingSignal()) source.recovery.signal();
      connect(source);
    }, Math.max(0, delayMs));
  };

  const connect = (source: PeerSource) => {
    if (!isCurrent(source) || source.stop) return;
    clearRetryTimer(source);
    const generation = source.recovery.beginConnection();
    source.generation = generation;
    try {
      const stop = ports.subscribe(source.peerId, source.recovery.currentCursor(), {
        frame: value => receiveFrame(source, generation, value),
        ready: () => {
          if (isCurrent(source) && source.recovery.isCurrent(generation)) {
            source.backoff.reset();
            refreshHistory(source);
          }
        },
        end: end => {
          if (!isCurrent(source) || !source.recovery.isCurrent(generation)) return;
          source.stop = undefined;
          if (end.reason === "abort") return;
          const reason: StreamEndReason = end.reason === "http"
            ? streamResponseFailureReason(end.status ?? 500)
            : end.reason === "eof" ? "eof" : "error";
          const decision = source.recovery.handleStreamEnd(generation, reason);
          if (decision === "reconnect") {
            connect(source);
            return;
          }
          if (decision === "wait" && end.reason === "http" && ((end.status ?? 0) >= 400 && (end.status ?? 0) < 500 || end.status === 503)) {
            const step = source.backoff.recordRefusal(end.retryAfterMs);
            scheduleRetry(source, step.delayMs);
          }
        },
      });
      if (isCurrent(source) && source.recovery.isCurrent(generation)) source.stop = stop;
      else stop();
    } catch {
      if (source.recovery.isCurrent(generation)) source.recovery.handleStreamEnd(generation, "error");
    }
  };

  const stopStream = (source: PeerSource) => {
    clearRetryTimer(source);
    if (!source.stop) return;
    const stop = source.stop;
    source.stop = undefined;
    source.recovery.handleStreamEnd(source.generation, "error");
    stop();
  };

  const disposeSource = (source: PeerSource) => {
    clearRetryTimer(source);
    source.recovery.dispose();
    source.loadController?.abort();
    source.stop?.();
    source.stop = undefined;
  };

  const createSource = (peerId: string, peerName: string, publicKey: string): PeerSource => ({
    peerId,
    peerName,
    publicKey,
    recovery: createRemoteChannelRecovery<RemoteNotificationCursor>({
      channel: "control",
      advanced: (previous, next) => previous === null || previous.boot !== next.boot || next.seq > previous.seq,
    }),
    backoff: createPeerStreamBackoff(ports.backoff),
    retryTimer: null,
    generation: 0,
    loading: false,
    bufferOverflowed: false,
    refreshAgain: false,
    buffered: [],
  });

  return {
    captureRoute(peerId: string, publicKey: string): RemoteNotificationRouteLease | undefined {
      const source = sources.get(peerId);
      if (!source || !isCurrent(source) || source.publicKey !== publicKey) return;
      return {
        isCurrent: () => isCurrent(source),
        confirm: action => { if (isCurrent(source)) applyActions(source, [action]); },
      };
    },
    sync(roster: PairedRoster | null) {
      if (disposed) return;
      const peers = new Map((roster?.peers?.paired ?? [])
        .filter(peer => peer.instance_id.trim() && peer.public_key.trim())
        .map(peer => [peer.instance_id, peer]));
      for (const [peerId, source] of sources) {
        const peer = peers.get(peerId);
        if (!peer || peer.public_key !== source.publicKey) {
          disposeSource(source);
          sources.delete(peerId);
          dispatch({ type: "REPLACE_REMOTE_NOTIFICATION_HISTORY", peerId, notifications: [] });
          continue;
        }
        if (source.peerName !== peer.instance_name) {
          source.peerName = peer.instance_name;
          refreshHistory(source);
        }
        const available = pairedAvailability(peer, roster?.peers.installations ?? []) === "reconnecting";
        if (!available) stopStream(source);
        else if (!source.stop) {
          const remaining = source.backoff.remainingMs();
          if (remaining > 0) {
            scheduleRetry(source, remaining);
          } else {
            if (source.recovery.isAwaitingSignal()) source.recovery.signal();
            connect(source);
          }
        }
      }
      for (const peer of peers.values()) {
        if (!peer.instance_id || sources.has(peer.instance_id)) continue;
        const source = createSource(peer.instance_id, peer.instance_name, peer.public_key);
        sources.set(peer.instance_id, source);
        if (pairedAvailability(peer, roster?.peers.installations ?? []) === "reconnecting") connect(source);
      }
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      for (const source of sources.values()) disposeSource(source);
      sources.clear();
    },
  };
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null ? value as Record<string, unknown> : null;
}
