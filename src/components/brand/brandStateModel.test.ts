import assert from "node:assert/strict";
import { test } from "node:test";
import {
  RECONNECT_GRACE_MS,
  RECONNECTED_NOTICE_MS,
  brandConnectionModel,
  brandStateModel,
  transitionBrandConnection,
} from "./brandStateModel";

test("disconnect remains hidden until the exact grace deadline", () => {
  const disconnected = transitionBrandConnection({ kind: "connected" }, "disconnect", 500);
  assert.deepEqual(brandConnectionModel(disconnected, 500 + RECONNECT_GRACE_MS - 1), { kind: "hidden" });
  assert.deepEqual(brandConnectionModel(disconnected, 500 + RECONNECT_GRACE_MS), { kind: "reconnecting" });
  assert.equal(transitionBrandConnection(disconnected, "grace-expired", 500 + RECONNECT_GRACE_MS).kind, "disconnected");
});

test("a short disconnect reconnects quietly and cancels the pending deadline", () => {
  const waiting = transitionBrandConnection({ kind: "connected" }, "disconnect", 100);
  const connected = transitionBrandConnection(waiting, "connect", 9_999);
  const lateDeadline = transitionBrandConnection(connected, "grace-expired", 10_100);
  assert.equal(lateDeadline.kind, "connected");
  assert.deepEqual(brandConnectionModel(connected, 9_999), { kind: "hidden" });
});

test("a disconnect that outlasts the grace period shows a reconnect notice", () => {
  const waiting = transitionBrandConnection({ kind: "connected" }, "disconnect", 100);
  const reconnected = transitionBrandConnection(waiting, "connect", 100 + RECONNECT_GRACE_MS);
  assert.deepEqual(reconnected, { kind: "reconnected", connectedAt: 100 + RECONNECT_GRACE_MS });
  assert.deepEqual(brandConnectionModel(reconnected, 100 + RECONNECT_GRACE_MS), { kind: "reconnected" });
  assert.deepEqual(brandConnectionModel(reconnected, 100 + RECONNECT_GRACE_MS + RECONNECTED_NOTICE_MS), { kind: "hidden" });
});

test("error presentation takes precedence over loading", () => {
  assert.deepEqual(brandStateModel({
    loading: true,
    label: "Loading",
    error: { title: "Could not start", detail: "Retry the session." },
  }), { kind: "error", title: "Could not start", detail: "Retry the session." });
  assert.deepEqual(brandStateModel({ loading: true, label: "Restoring workspace" }), {
    kind: "loading",
    label: "Restoring workspace",
    animate: true,
  });
});
