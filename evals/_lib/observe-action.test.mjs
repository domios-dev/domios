import assert from "node:assert/strict";
import test from "node:test";
import { observeAction } from "./observe-action.mjs";

test("observation is armed before an action can synchronously complete its event", async () => {
  let armed = false;
  let deliver;
  const event = new Promise(resolve => { deliver = resolve; });
  const result = await observeAction(() => {
    armed = true;
    return event;
  }, () => {
    assert.equal(armed, true);
    deliver("supplied-event");
  });
  assert.equal(result, "supplied-event");
});

test("action failures remain failures after the observer is armed", async () => {
  const failure = new Error("supplied-failure");
  await assert.rejects(observeAction(() => Promise.resolve("unused"), () => { throw failure; }),
    error => error === failure);
});
