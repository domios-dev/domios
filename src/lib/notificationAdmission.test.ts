import assert from "node:assert/strict";
import { test } from "node:test";
import { admitBellNotification, notificationDestination } from "./notificationAdmission";
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
