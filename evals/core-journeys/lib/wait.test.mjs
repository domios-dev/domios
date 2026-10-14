import assert from "node:assert/strict";
import test from "node:test";
import { JourneyFailure, observe, touchesPlan, touchesTab } from "./wait.mjs";

function fakeEvents(script) {
  const queue = [...script];
  return {
    drained: 0,
    next() {
      if (!queue.length) return Promise.reject(new Error("deadline"));
      return Promise.resolve(queue.shift());
    },
    drain() { this.drained += 1; return []; },
  };
}

test("observe re-reads after a matching event and returns the verdict", async () => {
  const reads = [{ n: 1 }, { n: 2 }];
  const events = fakeEvents([{ event_type: "noise" }, { event_type: "mux-state-changed" }]);
  const value = await observe({
    events, filter: (event) => event.event_type === "mux-state-changed",
    read: async () => reads.shift(), judge: (snapshot) => (snapshot.n === 2 ? { done: "ok" } : { pending: "n=2" }),
  });
  assert.equal(value, "ok");
  assert.equal(events.drained, 1);
});

test("observe turns the event deadline into a failure naming the pending condition", async () => {
  await assert.rejects(
    observe({ events: fakeEvents([]), read: async () => ({}), judge: () => ({ pending: "Worker ACK" }) }),
    (error) => error instanceof JourneyFailure && error.reason === "deadline: Worker ACK",
  );
});

test("observe fails immediately on a judged failure and retries a 503 read on the next event", async () => {
  let calls = 0;
  const read = async () => {
    calls += 1;
    if (calls === 1) throw Object.assign(new Error("busy"), { status: 503 });
    return { bad: true };
  };
  await assert.rejects(observe({ events: fakeEvents([{}]), read, judge: () => ({ fail: "SizeLimit" }) }), /SizeLimit/);
  assert.equal(calls, 2);
});

test("event filters match their subject", () => {
  assert.ok(touchesTab("t1")({ data: { tab_id: "t1" } }));
  assert.ok(!touchesTab("t1")({ data: { tab_id: "t2" } }));
  assert.ok(touchesPlan("p")({ event_type: "plan-changed" }));
  assert.ok(!touchesPlan("p")({ event_type: "tab-output" }));
});
