import assert from "node:assert/strict";
import { test } from "node:test";
import { isNewLaunchBound, waitForNewLaunchBinding, type BindingSnapshot } from "./live-switch-restore";

const prior = { session: "own", nonce: "old", pty: "old-pty" };
const bound: BindingSnapshot = {
  agent_session_id: "own", session_binding_nonce: "new", reported_session_binding_nonce: "new", pty_id: "new-pty",
  lifecycle: { transitions: [{ transition: { transition: "session_bound", session_id: "own", binding_nonce: "new" } }] },
};

test("restore requires fresh exact-session bound transition and matching reported launch", () => {
  assert.equal(isNewLaunchBound(bound, prior), true);
  for (const snapshot of [
    { ...bound, session_binding_nonce: "old" }, { ...bound, pty_id: "old-pty" },
    { ...bound, agent_session_id: "foreign" }, { ...bound, reported_session_binding_nonce: "old" },
    { ...bound, lifecycle: { transitions: [] } },
  ]) assert.equal(isNewLaunchBound(snapshot, prior), false);
});

test("restore waits for binding wakeup instead of trusting persisted prior identity", async () => {
  let snapshot: BindingSnapshot = { ...bound, session_binding_nonce: "old" };
  let wake = () => {};
  let observed!: () => void;
  const firstRead = new Promise<void>(resolve => { observed = resolve; });
  let reads = 0;
  let unsubscribed = false;
  const result = waitForNewLaunchBinding(async () => { reads++; observed(); return snapshot; }, prior,
    listener => { wake = listener; return () => { unsubscribed = true; }; }, 1000);
  await firstRead;
  assert.equal(reads, 1);
  snapshot = bound;
  wake();
  assert.equal(await result, bound);
  assert.equal(unsubscribed, true);
});
