import assert from "node:assert/strict";
import { eventReconnectDelay, isLocalEventStream, LOCAL_EVENT_RETRY_MS, LOCAL_EVENT_RETRY_CAP_MS, nextEventReconnectBackoff } from "./webEventRecovery";
import {
  HEALTHY_STREAM_MS,
  RECONNECT_CAP_MS,
  RECONNECT_FIRST_MS,
  RECONNECT_HIDDEN_CAP_MS,
  RECONNECT_HIDDEN_FIRST_MS,
  nextReconnectBackoff,
  reconnectDelay,
  shouldResetReconnectBackoff,
  transitionWebReconnect as step,
} from "./webReconnectModel";

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
assert.equal(reconnectDelay(RECONNECT_CAP_MS, 0), RECONNECT_CAP_MS / 2);
assert.equal(reconnectDelay(RECONNECT_CAP_MS, 1), RECONNECT_CAP_MS);
assert.equal(reconnectDelay(RECONNECT_FIRST_MS, 0, true), RECONNECT_HIDDEN_FIRST_MS / 2);
assert.equal(reconnectDelay(RECONNECT_HIDDEN_CAP_MS, 1, true), RECONNECT_HIDDEN_CAP_MS);
let backoff = RECONNECT_FIRST_MS;
for (let attempt = 0; attempt < 10; attempt++) backoff = nextReconnectBackoff(backoff);
assert.equal(backoff, RECONNECT_CAP_MS);
assert.equal(nextReconnectBackoff(RECONNECT_FIRST_MS, true), RECONNECT_HIDDEN_FIRST_MS * 2);
assert.equal(nextReconnectBackoff(RECONNECT_HIDDEN_CAP_MS, true), RECONNECT_HIDDEN_CAP_MS);

assert.equal(shouldResetReconnectBackoff(true, 0), true, "a real frame proves the stream is healthy");
assert.equal(shouldResetReconnectBackoff(false, HEALTHY_STREAM_MS - 1), false);
assert.equal(shouldResetReconnectBackoff(false, HEALTHY_STREAM_MS), true, "a long-lived stream proves health");

function attemptsInWindow(windowMs: number, hidden: boolean): number {
  let attempts = 1;
  let elapsed = 0;
  let delayBase = RECONNECT_FIRST_MS;
  while (elapsed <= windowMs) {
    elapsed += reconnectDelay(delayBase, 0, hidden);
    if (elapsed > windowMs) break;
    attempts++;
    delayBase = nextReconnectBackoff(delayBase, hidden);
  }
  return attempts;
}

assert.ok(attemptsInWindow(60_000, false) <= 14, "an empty-EOF train is bounded over one minute");
assert.ok(attemptsInWindow(60_000, true) <= 4, "hidden pages use the longer bounded schedule");

for (const origin of ["http://localhost:7686", "http://127.0.0.1:7685", "http://[::1]:7686"]) {
  assert.equal(isLocalEventStream(origin, false), true);
  assert.equal(isLocalEventStream(origin, true), false, "relay streams keep remote outage bounds");
}
assert.equal(isLocalEventStream("http://192.0.2.1:7685", false), false);
for (const hidden of [false, true]) {
  assert.equal(eventReconnectDelay(LOCAL_EVENT_RETRY_MS, 0, hidden, true), 125);
  assert.equal(eventReconnectDelay(60_000, 1, hidden, true), LOCAL_EVENT_RETRY_CAP_MS);
  assert.equal(nextEventReconnectBackoff(60_000, hidden, true), LOCAL_EVENT_RETRY_CAP_MS);
  assert.equal(eventReconnectDelay(RECONNECT_FIRST_MS, 0, hidden, false), reconnectDelay(RECONNECT_FIRST_MS, 0, hidden));
}
console.log("Local event recovery policy passed");
