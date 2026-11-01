import assert from "node:assert/strict";
import { test } from "node:test";
import { createStartupSendGate } from "./startupSendGate";

test("elapsed startup time enables only explicit input; own-session readiness retires startup", () => {
  let clock = 100;
  const gate = createStartupSendGate(() => clock);
  gate.observe("launch-a", true);
  clock += 4_999;
  assert.equal(gate.remainingMs(), 1);
  clock++;
  assert.equal(gate.remainingMs(), 0);
  assert.equal(gate.isStarting(), true);
  gate.observe("launch-a", false);
  assert.equal(gate.isStarting(), false);
  assert.equal(gate.remainingMs(), 0);
  gate.observe("launch-b", true);
  assert.equal(gate.remainingMs(), 5_000);
});
