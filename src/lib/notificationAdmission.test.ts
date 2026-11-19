import assert from "node:assert/strict";
import { test } from "node:test";
import { admitBellNotification, notificationDestination } from "./notificationAdmission";
import { projectRemoteNotification, remoteNotificationRoute } from "./remoteNotifications";
import type { Notification } from "./notificationTypes";

const notice: Notification = { id: "one", title: "opaque", body: null, severity: "Info", tab_id: "agent", workspace_id: null, timestamp: 1, read: false };

test("source routes diagnostics, plugin progress and local notices outside the bell", () => {
  assert.equal(notificationDestination("diagnostic"), "log");
  assert.equal(notificationDestination("plugin_progress"), "activity");
  assert.equal(notificationDestination("local_notice"), "toast");
  for (const source of ["diagnostic", "plugin_progress", "local_notice"] as const) {
    for (const level of [undefined, "verbose", "action_needed", "failure"] as const) {
      assert.deepEqual(admitBellNotification([], { ...notice, level, severity: "Error" }, source), []);
    }
  }
});

test("explicit agent notices retain every declared level", () => {
  for (const level of [undefined, "verbose", "action_needed", "failure"] as const) {
    assert.equal(admitBellNotification([], { ...notice, level }, "agent_notification").length, 1);
  }
});

test("identical live notices cannot stack even when their ids differ", () => {
  const first = admitBellNotification([], notice, "agent_notification");
  assert.equal(admitBellNotification(first, { ...notice, id: "two", timestamp: 2 }, "agent_notification"), first);
  assert.equal(admitBellNotification(first, { ...notice, id: "two", tab_id: "other" }, "agent_notification").length, 2);
  assert.equal(admitBellNotification(first, { ...notice, body: "changed" }, "agent_notification")[0].body, "changed");
});

test("paired installations qualify bell identity and exact action routes", () => {
  const first = projectRemoteNotification("install-a", "Same name", { ...notice, id: "same-source" });
  const second = projectRemoteNotification("install-b", "Same name", { ...notice, id: "same-source" });
  assert.notEqual(first.id, second.id);
  assert.equal(first.tab_id, "mesh:install-a/agent");
  assert.equal(second.tab_id, "mesh:install-b/agent");
  assert.deepEqual(remoteNotificationRoute(first), { peerId: "install-a", sourceId: "same-source", peerName: "Same name" });

  const admitted = admitBellNotification([], first, "agent_notification");
  const withSecondOrigin = admitBellNotification(admitted, second, "agent_notification");
  assert.equal(withSecondOrigin.length, 2);
  const reconnect = admitBellNotification(withSecondOrigin, { ...first, timestamp: 2 }, "agent_notification");
  assert.equal(reconnect.length, 2);
  assert.equal(reconnect[0].timestamp, 2);
  assert.equal(remoteNotificationRoute({ ...first, id: "local-fallback" }), null);
  assert.equal(remoteNotificationRoute({ ...first, tab_id: "mesh:install-b/agent" }), null);
});

test("identical notifications without a tab still retain separate peer origins", () => {
  const source = { ...notice, tab_id: null };
  const a = projectRemoteNotification("a", "Same name", source);
  const b = projectRemoteNotification("b", "Same name", source);
  const admitted = [source, a, b].reduce<Notification[]>((rows, row) => admitBellNotification(rows, row, "agent_notification"), []);
  assert.deepEqual(admitted.map(row => row.id), [source.id, a.id, b.id]);
});
