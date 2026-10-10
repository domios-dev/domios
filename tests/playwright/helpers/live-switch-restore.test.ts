import assert from "node:assert/strict";
import { test } from "node:test";
import { isNewLaunchBound, restoredBindingEvidence, waitForNewLaunchBinding, type BindingSnapshot } from "./live-switch-restore";

const prior = { session: "own", nonce: "old", pty: "old-pty" };
const bound: BindingSnapshot = {
  agent_session_id: "own", session_binding_nonce: "new", reported_session_binding_nonce: "new", pty_id: "new-pty",
  boot_stage: "agent_ready", lifecycle_epoch: 3,
  lifecycle: { transitions: [{ transition: { transition: "session_bound", session_id: "own", binding_nonce: "new" } }] },
};

test("restore requires fresh exact-session bound transition and matching reported launch", () => {
  assert.equal(isNewLaunchBound(bound, prior), true);
  for (const snapshot of [
    { ...bound, session_binding_nonce: "old" }, { ...bound, pty_id: "old-pty" },
    { ...bound, agent_session_id: "foreign" }, { ...bound, reported_session_binding_nonce: "old" },
    { ...bound, lifecycle: { transitions: [] } }, { ...bound, boot_stage: "starting" },
  ]) assert.equal(isNewLaunchBound(snapshot, prior), false);
});

test("compacted authenticated checkpoint and structured binding retain exact launch guards", () => {
  const checkpoint: BindingSnapshot = { ...bound, lifecycle: { checkpoint: { key: { epoch: 3 },
    authority: { session_id: "own", binding_nonce: "new" },
    agent_type_write: { origin: "hook", nonce_verified: true, session_id: "own" } } } };
  assert.equal(restoredBindingEvidence(checkpoint, prior), "checkpoint binding (hook-authenticated)");
  assert.equal(isNewLaunchBound({ ...checkpoint, lifecycle_epoch: 4 }, prior), false);
  assert.equal(isNewLaunchBound({ ...checkpoint, session_binding_nonce: "foreign" }, prior), false);
  const structured = { ...bound, lifecycle: undefined, structured_control_active: true };
  assert.equal(restoredBindingEvidence(structured, prior), "structured binding (reported nonce)");
  assert.equal(isNewLaunchBound({ ...structured, reported_session_binding_nonce: "old" }, prior), false);
  assert.equal(isNewLaunchBound({ ...structured, agent_session_id: "foreign" }, prior), false);
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
