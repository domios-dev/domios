import assert from "node:assert/strict";
import { RECONNECT_CAP_MS, RECONNECT_FIRST_MS, nextReconnectBackoff, reconnectDelay, transitionWebReconnect as step } from "./webReconnectModel";

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

assert.equal(reconnectDelay(RECONNECT_FIRST_MS, 0), 500);
assert.equal(reconnectDelay(RECONNECT_FIRST_MS, 1), 1_000);
let backoff = RECONNECT_FIRST_MS;
for (let attempt = 0; attempt < 10; attempt++) backoff = nextReconnectBackoff(backoff);
assert.equal(backoff, RECONNECT_CAP_MS);
