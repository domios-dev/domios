import assert from "node:assert/strict";
import { transitionWebReconnect as step } from "./webReconnectModel";

for (const event of ["wake", "deadline"] as const) {
  assert.deepEqual(step("pending", event), { state: "probing", action: "probe" });
}
assert.deepEqual(step("probing", "wake"), { state: "probing", action: "none" });
assert.deepEqual(step("connected", "wake"), { state: "connected", action: "none" });
assert.deepEqual(step("probing", "probe-ok"), { state: "connecting", action: "connect" });
assert.deepEqual(step("probing", "probe-failed"), { state: "pending", action: "wait" });
assert.deepEqual(step("connected", "lost"), { state: "pending", action: "wait" });
assert.deepEqual(step("probing", "connected"), { state: "connected", action: "none" });
assert.deepEqual(step("pending", "stop"), { state: "stopped", action: "none" });
assert.deepEqual(step("stopped", "wake"), { state: "stopped", action: "none" });
assert.deepEqual(step("connecting", "probe-ok"), { state: "connecting", action: "none" });
assert.deepEqual(step("pending", "probe-ok"), { state: "pending", action: "none" });
console.log("12 passed, 0 failed");
