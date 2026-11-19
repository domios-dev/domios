import { describe, expect, it } from "vitest";
import type { Notification } from "../../lib/notificationTypes";
import { RELAY_PROTOCOL_VERSION, type MeshPeersResponse } from "../../lib/types";
import { createRemoteNotificationSource, MAX_BUFFERED_REMOTE_NOTIFICATION_ACTIONS, type RemoteNotificationEventHandlers, type RemoteNotificationSourceAction } from "./remoteNotificationSource";
import type { PairedRoster } from "./useMeshPeerProjection";

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
