import assert from "node:assert/strict";
import test from "node:test";
import { planOutputReconnect, type OutputReconnectState } from "./outputReconnect";

const state: OutputReconnectState = {
  failures: 0, connectedMs: 10, reason: "eof", current: true, subscribed: true,
};

test("real disconnects back off exponentially up to the bound", () => {
  for (const reason of ["eof", "error"] as const) {
    let failures = 0;
    const delays: number[] = [];
    for (let index = 0; index < 12; index++) {
      const plan = planOutputReconnect({ ...state, reason, failures })!;
      failures = plan.failures;
      delays.push(plan.delayMs);
    }
    assert.deepEqual(delays, [250, 500, 1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000, 30000, 30000]);
  }
});
test("abort, refusal, stale and detached streams never retry", () => {
  for (const change of [{ current: false }, { subscribed: false }, { reason: "abort" }, { reason: "refused" }] as const) {
    assert.equal(planOutputReconnect({ ...state, ...change }), null);
  }
});
test("a sustained connection resets disconnect backoff", () => {
  assert.deepEqual(planOutputReconnect({ ...state, failures: 8, connectedMs: 30_000 }), { failures: 1, delayMs: 250 });

});
