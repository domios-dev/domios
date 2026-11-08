import assert from "node:assert/strict";
import { test } from "node:test";
import { tabFailure, tabFailureDescription, type TabFailureFacts } from "./tabFailure";

const recoveryReason = "opaque: Interrupt completion deadline exceeded; runtime recovery required";
const degraded = { state: "degraded" as const, epoch: 1, failureCode: "provider_refused" as const, reason: recoveryReason };

const cases: Array<[string, TabFailureFacts, ReturnType<typeof tabFailure>["kind"], ReturnType<typeof tabFailure>["recovery"]]> = [
  ["a dead tab ended", { status: "Dead", launched: true, runtime: degraded }, "ended", null],
  ["a launch that never spawned", { bootStage: "spawn_failed", launched: false }, "launch_failed", "restart"],
  ["a restored launch that never spawned", { bootStage: "spawn_failed", launched: true }, "launch_failed", "restart"],
  ["a runtime that degraded before any session", { launched: false, runtime: degraded }, "launch_failed", "restart"],
  ["a launched session that degraded", { launched: true, runtime: degraded }, "runtime_needs_recovery", "restart"],
  ["a launched session whose runtime stopped", { launched: true, runtime: { state: "unloaded", epoch: 1, reason: "opaque" } }, "runtime_stopped", "restart"],
  ["a session the agent refused to load", { launched: true, runtime: { ...degraded, failureCode: "session_load_refused" } }, "session_unavailable", "start_new_session"],
  ["a start the runtime could not bind", { launched: false, runtime: { ...degraded, failureCode: "runtime_start_unbound" } }, "launch_failed", "start_new_session"],
  ["a failed relaunch of a session whose runtime degraded", { bootStage: "spawn_failed", launched: true, runtime: degraded }, "runtime_needs_recovery", "restart"],
  ["a failed relaunch of a session whose runtime stopped", { bootStage: "spawn_failed", launched: true, runtime: { state: "unloaded", epoch: 1, reason: "opaque" } }, "runtime_stopped", "restart"],
  ["a chat runtime that never bound a session", { bootStage: "spawn_failed", launched: false, chatBacked: true }, "launch_failed", "start_new_session"],
  ["a chat runtime that degraded before any session", { launched: false, chatBacked: true, runtime: degraded }, "launch_failed", "start_new_session"],
  ["a live agent the tracker reports in error", { status: "Error", launched: true }, "agent_error", "restart"],
  ["an error before the agent launched", { status: "Error", launched: false }, "launch_failed", "restart"],
];

test("each failed state names itself and its recovery", () => {
  for (const [name, facts, kind, recovery] of cases) {
    const failure = tabFailure(facts);
    assert.equal(failure.kind, kind, name);
    assert.equal(failure.recovery, recovery, name);
  }
});

test("the same state yields the same value, so activity selectors settle", () => {
  for (const [name, facts] of cases) assert.equal(tabFailure(facts), tabFailure({ ...facts }), name);
});

test("only a launch that never became ready reads as a launch failure", () => {
  const labels = new Map(cases.map(([, facts]) => [tabFailure(facts).kind, tabFailure(facts).label]));
  assert.equal(labels.get("launch_failed"), "Launch failed");
  assert.equal(labels.get("runtime_needs_recovery"), "Runtime needs recovery");
  assert.equal(labels.size, new Set(labels.values()).size, "each state has its own label");
});

test("a provider's wording never changes a typed runtime failure", () => {
  for (const reason of [recoveryReason, "Deadline: opaque", "Runtime did not respond in time: opaque"]) {
    const failure = tabFailure({ launched: true, runtime: { ...degraded, reason } });
    assert.equal(failure.kind, "runtime_needs_recovery");
    const notice = tabFailureDescription(failure, reason);
    assert.equal(notice?.title, failure.label);
    assert.equal(notice?.raw, reason, "the provider's words stay in Details");
    assert.equal(notice?.pending, undefined);
  }
});

test("only runtime failures replace the specific launch and agent descriptions", () => {
  assert.equal(tabFailureDescription(tabFailure({ bootStage: "spawn_failed", launched: false }), "opaque"), null);
  assert.equal(tabFailureDescription(tabFailure({ status: "Dead", launched: true }), "opaque"), null);
});
