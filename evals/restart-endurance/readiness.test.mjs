import test from "node:test";
import assert from "node:assert/strict";
import { restoreReady, reconcileReadiness } from "./readiness.mjs";

const fixture = { id: "owned", has_answer: true, first_turn_policy: {} };
const ready = { id: "owned", agent_type: "test", agent_session_id: "session", boot_stage: "agent_ready" };

test("a final ready snapshot reconciles a stream gap without mutating its evidence", () => {
  const observations = [{ id: "owned", readiness_error: "stream gap" }];
  const reconciled = reconcileReadiness(observations, [ready], [fixture]);
  assert.deepEqual(reconciled, [{ ...ready, recovered_readiness_error: "stream gap" }]);
  assert.deepEqual(observations, [{ id: "owned", readiness_error: "stream gap" }]);
});

test("reattachment alone cannot reconcile a launch readiness failure", () => {
  const observations = [{ id: "owned", readiness_error: "deadline" }];
  assert.deepEqual(reconcileReadiness(observations, [{ ...ready, boot_stage: "pty_reattached" }], [fixture]), observations);
  assert.equal(restoreReady({ ...ready, boot_stage: "pty_reattached" }, fixture), false);
});

test("an answered fixture needs its own bound session even after a stream gap", () => {
  const observations = [{ id: "owned", readiness_error: "deadline" }];
  const unbound = { ...ready, agent_session_id: null, pty_id: "pty", session_binding_nonce: "nonce" };
  const lazy = { ...fixture, first_turn_policy: { lazy: true, structuralComposer: true } };
  assert.deepEqual(reconcileReadiness(observations, [unbound], [lazy]), observations);
  assert.equal(restoreReady(unbound, { ...lazy, has_answer: false }), true);
});

test("another fixture's ready record never replaces a missing owned fixture", () => {
  const observations = [{ id: "owned", readiness_error: "absent" }];
  assert.deepEqual(reconcileReadiness(observations, [{ ...ready, id: "other" }], [fixture]), observations);
});
