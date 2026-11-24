import { afterEach, describe, expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import type { Notification } from "../../lib/notificationTypes";
import { RELAY_PROTOCOL_VERSION, type MeshPeersResponse } from "../../lib/types";
import { createRemoteNotificationSource, MAX_BUFFERED_REMOTE_NOTIFICATION_ACTIONS, type RemoteNotificationEventHandlers, type RemoteNotificationSourceAction } from "./remoteNotificationSource";
import type { PairedRoster } from "./useMeshPeerProjection";
import { reducer, type Action } from "./reducer";
import type { ExtendedState } from "./tree";
import { createMuxNotificationActions } from "./useMuxNotificationActions";
import { projectRemoteNotification } from "../../lib/remoteNotifications";
import { MAX_BELL_NOTIFICATIONS } from "../../lib/notificationAdmission";

afterEach(() => {
  __setInvokeForTest(null);
  vi.restoreAllMocks();
});

function peer(instanceId: string, publicKey: string): MeshPeersResponse["paired"][number] {
  return {
    instance_id: instanceId,
    instance_name: "Same name",
    addresses: [],
    api_port: 7685,
    public_key: publicKey,
    paired_at: "2026-01-01T00:00:00Z",
    last_seen: null,
    online: true,
    is_primary: false,
    access_level: "full",
    relay_protocol_version: RELAY_PROTOCOL_VERSION,
    web_ui: null,
    last_active_ms: null,
  };
}

function roster(peers: MeshPeersResponse["paired"]): PairedRoster {
  return { revision: 1, peers: { paired: peers, installations: [], local_instance_name: "Local" } as unknown as MeshPeersResponse };
}

function notification(title: string, id = "same-source-id"): Notification {
  return { id, title, body: "Waiting", severity: "Warning", level: "action_needed", tab_id: "main", workspace_id: null, read: false, timestamp: 1 };
}

function frame(handlers: RemoteNotificationEventHandlers, eventType: string, seq: number, data: unknown): void {
  handlers.frame({ event_type: eventType, data, boot: "boot-a", seq, prev_seq: seq - 1 });
}

function flushMicrotasks(): Promise<void> {
  return Promise.resolve().then(() => Promise.resolve()).then(() => Promise.resolve());
}

describe("remote notification source", () => {
  it("bounds concurrent remote reads and deletes and releases capacity on a receipt", async () => {
    const paired = roster([peer("install-a", "key-a")]);
    const rows = Array.from({ length: 2 * MAX_BELL_NOTIFICATIONS + 1 }, (_, index) =>
      projectRemoteNotification("install-a", "Peer", notification("Opaque", String(index))));
    const completions: Array<() => void> = [];
    __setInvokeForTest(<T,>() => new Promise<T>(resolve => completions.push(() => resolve({} as T))));
    const actions = createMuxNotificationActions({
      notifications: () => rows,
      roster: () => paired,
      dispatch: () => {},
      captureRoute: () => ({ isCurrent: () => true, confirm: () => {} }),
    });
    const pending = rows.slice(0, -1).map(row => actions.markRead(row.id));
    const extra = rows.at(-1)!;
    expect(actions.deleteNotification(extra.id)).toBeUndefined();
    expect(completions).toHaveLength(2 * MAX_BELL_NOTIFICATIONS);
    completions[0]();
    await pending[0];
    const next = actions.deleteNotification(extra.id);
    expect(completions).toHaveLength(2 * MAX_BELL_NOTIFICATIONS + 1);
    completions.forEach(complete => complete());
    await Promise.all([...pending, next]);
  });

  it("orders exact action receipts after peer history and leaves denied rows untouched", async () => {
    const a = peer("install-a", "key-a");
    const b = peer("install-b", "key-b");
    let paired = roster([a, b]);
    let state: ExtendedState = {
      workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
      notifications: [notification("Local")], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
    };
    const dispatch = (action: Action) => { state = reducer(state, action); };
    const idA = "remote-notification/install-a/same-source-id";
    const idB = "remote-notification/install-b/same-source-id";
    const loads = new Map<string, (rows: Notification[]) => void>();
    const handlers = new Map<string, RemoteNotificationEventHandlers>();
    const source = createRemoteNotificationSource(dispatch, {
      load(peerId) { return new Promise(resolve => loads.set(peerId, resolve)); },
      subscribe(peerId, _cursor, handler) {
        handlers.set(peerId, handler);
        return () => undefined;
      },
    });
    const actions = createMuxNotificationActions({
      notifications: () => state.notifications,
      roster: () => paired,
      dispatch,
      captureRoute: source.captureRoute,
    });
    const calls: Array<[string, Record<string, unknown> | undefined]> = [];
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    __setInvokeForTest(async <T>(command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      if (args?.peerId === "install-b") throw new Error("Refused");
      return {} as T;
    });

    source.sync(paired);
    for (const peerId of ["install-a", "install-b"]) {
      handlers.get(peerId)!.ready();
      loads.get(peerId)!([notification("Same notice")]);
    }
    await flushMicrotasks();
    expect(state.notifications.map(row => row.id)).toEqual(["same-source-id", idA, idB]);

    handlers.get("install-a")!.ready();
    await actions.markRead(idA);
    loads.get("install-a")!([notification("Stale unread A")]);
    await flushMicrotasks();
    expect(state.notifications.find(row => row.id === idA)?.read).toBe(true);
    expect(state.notifications.find(row => row.id === idB)?.read).toBe(false);

    await actions.markAllRead();
    expect(state.notifications.find(row => row.id === "same-source-id")?.read).toBe(true);
    expect(state.notifications.find(row => row.id === idB)?.read).toBe(false);

    handlers.get("install-a")!.ready();
    await actions.clearNotifications();
    loads.get("install-a")!([notification("Stale retained A")]);
    await flushMicrotasks();
    expect(state.notifications.map(row => row.id)).toEqual([idB]);
    expect(calls).toEqual([
      ["mesh_peer_notification_action", { peerId: "install-a", sourceId: "same-source-id", notificationAction: "read" }],
      ["mesh_peer_notification_action", { peerId: "install-b", sourceId: "same-source-id", notificationAction: "read" }],
      ["mark_notification_read", { id: "same-source-id" }],
      ["mesh_peer_notification_action", { peerId: "install-b", sourceId: "same-source-id", notificationAction: "delete" }],
      ["mesh_peer_notification_action", { peerId: "install-a", sourceId: "same-source-id", notificationAction: "delete" }],
      ["clear_notifications", undefined],
    ]);

    const oldB = source.captureRoute("install-b", "key-b")!;
    paired = roster([a]);
    source.sync(paired);
    paired = roster([a, b]);
    source.sync(paired);
    handlers.get("install-b")!.ready();
    oldB.confirm({ type: "MARK_READ", id: idB });
    loads.get("install-b")!([notification("Replacement B")]);
    await flushMicrotasks();
    expect(state.notifications).toEqual([expect.objectContaining({ id: idB, read: false, title: "Replacement B" })]);
    source.dispose();
  });

  it.each(["answer first", "history first"])("retains a remote answer with stale history and no answer event (%s)", async (order) => {
    const paired = roster([peer("install-a", "key-a")]);
    let state: ExtendedState = {
      workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
      notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 1, pendingCloses: [],
    };
    const dispatch = (action: Action) => { state = reducer(state, action); };
    const loads: Array<(rows: Notification[]) => void> = [];
    let ready!: () => void;
    const source = createRemoteNotificationSource(dispatch, {
      load() { return new Promise(resolve => loads.push(resolve)); },
      subscribe(_peerId, _cursor, handlers) { ready = handlers.ready; return () => undefined; },
    });
    const actions = createMuxNotificationActions({
      notifications: () => state.notifications,
      roster: () => paired,
      dispatch,
      captureRoute: source.captureRoute,
    });
    const original: Notification = {
      ...notification("Waiting"),
      answer: { asked_from_tab: "main", answered: null, answered_via: null, delivered: false },
    };
    const calls: Array<[string, Record<string, unknown> | undefined]> = [];
    let completeAnswer!: (result: { answered: string; delivered: boolean }) => void;
    const response = new Promise<{ answered: string; delivered: boolean }>(resolve => { completeAnswer = resolve; });
    __setInvokeForTest(async <T>(command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      return await response as T;
    });

    source.sync(paired);
    ready();
    loads[0]([original]);
    await flushMicrotasks();
    const id = state.notifications[0].id;
    const row = () => state.notifications.find(item => item.id === id);

    ready();
    const answer = actions.answerNotification(id, "Approve");
    expect(row()?.answer_pending).toBe(true);
    if (order === "history first") {
      loads[1]([original]);
      await flushMicrotasks();
      expect(row()?.answer_pending).toBe(true);
      await actions.answerNotification(id, "Approve");
    }
    completeAnswer({ answered: "Approve", delivered: true });
    await answer;
    if (order === "answer first") {
      loads[1]([original]);
      await flushMicrotasks();
    }

    expect(calls).toEqual([["mesh_peer_notification_action", {
      peerId: "install-a", sourceId: "same-source-id", notificationAction: "answer", label: "Approve",
    }]]);
    expect(row()).toMatchObject({ read: true, answer_pending: false, answer: { answered: "Approve", delivered: true } });
    source.dispose();
  });

  it("qualifies colliding peer items, applies buffered events after history, resumes from cursor, and clears revoked routes", async () => {
    const actions: RemoteNotificationSourceAction[] = [];
    const subscriptions = new Map<string, RemoteNotificationEventHandlers[]>();
    const stops: string[] = [];
    const arrivals: string[] = [];
    const pendingLoads = new Map<string, Array<{ signal: AbortSignal; resolve: (rows: ReturnType<typeof notification>[]) => void }>>();
    const cursors: Array<{ peerId: string; cursor: { boot: string; seq: number } | null }> = [];
    const source = createRemoteNotificationSource(action => actions.push(action), {
      load(peerId, signal) {
        return new Promise(resolve => {
          const rows = pendingLoads.get(peerId) ?? [];
          rows.push({ signal, resolve });
          pendingLoads.set(peerId, rows);
        });
      },
      arrival: notification => arrivals.push(notification.id),
      subscribe(peerId, cursor, handlers) {
        const peerSubscriptions = subscriptions.get(peerId) ?? [];
        peerSubscriptions.push(handlers);
        subscriptions.set(peerId, peerSubscriptions);
        cursors.push({ peerId, cursor });
        return () => stops.push(peerId);
      },
    });

    const peerA = peer("install-a", "key-a");
    const peerB = peer("install-b", "key-b");
    source.sync(roster([peerA, peerB]));
    const firstA = subscriptions.get("install-a")![0];
    const firstB = subscriptions.get("install-b")![0];
    firstA.ready();
    firstB.ready();
    frame(firstA, "notification-created", 1, notification("Live A"));
    frame(firstA, "notification-updated", 2, { id: "same-source-id", title: "Latest A", body: "Still waiting" });
    frame(firstB, "notification-created", 1, notification("Live B"));

    pendingLoads.get("install-a")![0].resolve([notification("History A")]);
    pendingLoads.get("install-b")![0].resolve([notification("History B")]);
    await flushMicrotasks();

    const historyA = actions.find(action => action.type === "REPLACE_REMOTE_NOTIFICATION_HISTORY" && action.peerId === "install-a");
    const historyB = actions.find(action => action.type === "REPLACE_REMOTE_NOTIFICATION_HISTORY" && action.peerId === "install-b");
    expect(historyA?.type === "REPLACE_REMOTE_NOTIFICATION_HISTORY" && historyA.notifications[0].id).toBe("remote-notification/install-a/same-source-id");
    expect(historyB?.type === "REPLACE_REMOTE_NOTIFICATION_HISTORY" && historyB.notifications[0].id).toBe("remote-notification/install-b/same-source-id");
    const historyIndex = actions.indexOf(historyA!);
    const addIndex = actions.findIndex(action => action.type === "ADD_NOTIFICATION" && action.notification.remote_origin?.peerId === "install-a");
    const updateIndex = actions.findIndex(action => action.type === "UPDATE_NOTIFICATION_SUMMARY" && action.id === "remote-notification/install-a/same-source-id");
    expect(historyIndex).toBeLessThan(addIndex);
    expect(addIndex).toBeLessThan(updateIndex);
    expect(arrivals).toEqual([
      "remote-notification/install-a/same-source-id",
      "remote-notification/install-b/same-source-id",
    ]);

    const beforeReplay = actions.filter(action => action.type === "ADD_NOTIFICATION").length;
    frame(firstA, "notification-created", 2, notification("Replayed A"));
    expect(actions.filter(action => action.type === "ADD_NOTIFICATION")).toHaveLength(beforeReplay);
    expect(arrivals).toHaveLength(2);

    firstA.end({ reason: "eof" });
    const resumedA = subscriptions.get("install-a")![1];
    expect(cursors.filter(entry => entry.peerId === "install-a").pop()?.cursor).toEqual({ boot: "boot-a", seq: 2 });
    resumedA.ready();
    frame(resumedA, "notification-created", 2, notification("Reconnect replay"));
    pendingLoads.get("install-a")![1].resolve([notification("History A")]);
    await flushMicrotasks();
    expect(actions.filter(action => action.type === "ADD_NOTIFICATION")).toHaveLength(beforeReplay);
    expect(arrivals).toHaveLength(2);

    source.sync(roster([peerA]));
    expect(stops).toContain("install-b");
    expect(actions[actions.length - 1]).toEqual({ type: "REPLACE_REMOTE_NOTIFICATION_HISTORY", peerId: "install-b", notifications: [] });
    const beforeRevokedFrame = actions.length;
    frame(firstB, "notification-created", 2, notification("Revoked"));
    expect(actions).toHaveLength(beforeRevokedFrame);

    source.sync(null);
    expect(actions[actions.length - 1]).toEqual({ type: "REPLACE_REMOTE_NOTIFICATION_HISTORY", peerId: "install-a", notifications: [] });
    source.dispose();
  });

  it("bounds live actions during a slow history read and reloads without applying stale state", async () => {
    const actions: RemoteNotificationSourceAction[] = [];
    const arrivals: string[] = [];
    const loads: Array<(rows: Notification[]) => void> = [];
    let handlers: RemoteNotificationEventHandlers | undefined;
    const source = createRemoteNotificationSource(action => actions.push(action), {
      load() {
        return new Promise(resolve => loads.push(resolve));
      },
      arrival: notification => arrivals.push(notification.id),
      subscribe(_peerId, _cursor, nextHandlers) {
        handlers = nextHandlers;
        return () => undefined;
      },
    });

    source.sync(roster([peer("install-a", "key-a")]));
    handlers!.ready();
    for (let seq = 1; seq <= MAX_BUFFERED_REMOTE_NOTIFICATION_ACTIONS + 1; seq++) {
      handlers!.frame({
        event_type: "notification-created",
        data: notification(`Live ${seq}`, `source-${seq}`),
        boot: "boot-a",
        seq,
        prev_seq: seq - 1,
      });
    }
    loads[0]([notification("Stale", "stale")]);
    await flushMicrotasks();
    expect(loads).toHaveLength(2);
    expect(actions).toEqual([]);
    expect(arrivals).toHaveLength(MAX_BUFFERED_REMOTE_NOTIFICATION_ACTIONS + 1);

    loads[1]([notification("Current", "current")]);
    await flushMicrotasks();
    expect(actions).toEqual([{
      type: "REPLACE_REMOTE_NOTIFICATION_HISTORY",
      peerId: "install-a",
      notifications: [expect.objectContaining({ id: "remote-notification/install-a/current" })],
    }]);
    source.dispose();
  });
});
