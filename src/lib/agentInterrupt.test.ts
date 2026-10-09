import assert from "node:assert/strict";
import test from "node:test";
import { interruptAgentArgs } from "./agentInterrupt";

test("ordinary tab interruption retains provider authority", () => {
  assert.deepEqual(interruptAgentArgs({ id: "ordinary", pty_id: "pty" }), { ptyId: "pty", tabId: "ordinary" });
});

test("tabs without a PTY cannot interrupt", () => {
  assert.equal(interruptAgentArgs({ id: "ordinary", pty_id: null }), null);
});
