import assert from "node:assert/strict";
import { restorePanelOwnsBody, restoreStageView, transcriptLoadingStageView } from "./restoreStageView";
import type { Tab } from "./types";

const tab: Partial<Tab> = { agent_type: "fixture", agent_session_id: "saved", frozen: true,
  boot_stage: "agent_ready", agent_runtime_state: { state: "loaded", epoch: 1 } };
assert.equal(restoreStageView(tab).phase, "sleeping");
for (const frozen of [true, false]) {
  const shell: Partial<Tab> = { backing: { kind: "pty" }, frozen, boot_stage: "pty_starting" };
  assert.equal(restorePanelOwnsBody(shell, { requested: true }, restoreStageView(shell)), false);
}
const queued = restoreStageView(tab, { requested: true });
assert.equal(queued.phase, "restoring");
assert.equal(queued.current, -1);
assert(queued.steps.every(step => step.state === "pending"));
const awake = { ...tab, frozen: false };
const firstTurnTab: Partial<Tab> = {
  agent_type: "lazy-fixture", backing: { kind: "pty" }, agent_session_id: null,
  frozen: false, agent_booting: false, boot_stage: "provider_ready",
  pty_id: "own-pty", session_binding_nonce: "own-nonce",
  provisional_launch: { pty_id: "own-pty", binding_nonce: "own-nonce", reason: "opaque",
    awaiting_binding_ack: true, first_turn_input_ready: true, first_turn_payload_written: false },
};
assert.equal(restoreStageView(firstTurnTab).phase, "ready");
assert.equal(restorePanelOwnsBody(firstTurnTab, { requested: true }, restoreStageView(firstTurnTab)), false);
for (const changed of [
  { ...firstTurnTab, session_binding_nonce: "foreign-nonce" },
  { ...firstTurnTab, pty_id: "foreign-pty" },
  { ...firstTurnTab, agent_booting: true },
]) assert.equal(restoreStageView(changed).phase, "restoring");
assert.equal(restoreStageView({ ...firstTurnTab, frozen: true }).phase, "sleeping");
for (const [boot_stage, current] of [["pty_starting", 0], ["reader_ready", 0], ["agent_launching", 1], ["launch_written", 1], ["provider_ready", 2], ["launch_binding_unconfirmed", 2]] as const) {
  const view = restoreStageView({ ...awake, boot_stage });
  assert.equal(view.phase, "restoring");
  assert.equal(view.current, current);
}
assert.equal(restoreStageView(awake).phase, "ready");
const signInTab: Partial<Tab> = { ...awake, agent_runtime_state: { state: "loginPending", epoch: 3, auth: "signedOut", loginMethods: ["browser"] } };
const signInView = restoreStageView(signInTab, { requested: true });
assert.equal(signInView.phase, "attention");
assert.equal(restorePanelOwnsBody(signInTab, { requested: true }, signInView), false);
const transcriptPending = transcriptLoadingStageView(awake);
assert.equal(transcriptPending.phase, "restoring");
assert.equal(transcriptPending.steps[transcriptPending.steps.length - 1]?.state, "pending");
assert.equal(transcriptPending.steps[transcriptPending.current].state, "current");
assert.equal(restoreStageView(awake, { error: "RPC response lost" }).phase, "ready");
assert(restoreStageView(awake).steps.every(step => step.state === "complete"));
assert.equal(restoreStageView({ ...awake, boot_stage: "awaiting_owner_prompt" }).phase, "attention");
const failed = restoreStageView({ ...awake, boot_stage: "spawn_failed", boot_failure_reason: "opaque-error" });
assert.equal(failed.phase, "failed");
assert.equal(failed.detail, "opaque-error");
assert.equal(restorePanelOwnsBody({ ...awake, boot_stage: "spawn_failed" }, { requested: true }, failed), false);
assert.equal(restoreStageView(tab, { error: "RPC error" }).detail, "RPC error");
assert.equal(restoreStageView({ ...awake, boot_stage: "spawn_failed" }, { pending: true }).phase, "restoring");
const runtime: Partial<Tab> = { backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "saved" }, agent_runtime_state: { state: "loading", epoch: 2 } };
assert.equal(restoreStageView(runtime).current, 0);
assert.equal(restoreStageView({ ...runtime, boot_stage: "agent_launching" }).current, 0);
assert.equal(restoreStageView({ ...runtime, agent_runtime_state: { state: "loaded", epoch: 2 } }).phase, "ready");
assert.equal(restoreStageView({ ...runtime, agent_runtime_state: { state: "degraded", reason: "native-error", epoch: 2 } }).detail, "native-error");
const deadlineRuntime = { ...runtime, backing: { kind: "chat_backend" as const, plugin_id: "fixture", session_id: "saved" },
  boot_stage: "agent_ready" as const, session_bound: true,
  agent_runtime_state: { state: "degraded" as const, reason: "Deadline: opaque", epoch: 2 } };
const deadlineView = restoreStageView(deadlineRuntime);
assert.equal(deadlineView.phase, "restoring");
assert.equal(deadlineView.current, 2);
assert(deadlineView.hint);
const recoveredView = restoreStageView({ ...deadlineRuntime, agent_runtime_state: { state: "loaded" as const, epoch: 3 } });
assert.equal(recoveredView.phase, "ready");
assert.equal(recoveredView.hint, null);
const terminalRuntime = restoreStageView({ ...deadlineRuntime,
  agent_runtime_state: { state: "degraded" as const, reason: "ProcessExited: runtime exited", epoch: 3 } });
assert.equal(terminalRuntime.phase, "failed");
assert.equal(restoreStageView({ boot_stage: "shell_ready" }).phase, "ready");
assert.equal(restoreStageView({ boot_stage: "pty_spawned" }).phase, "restoring");
assert.equal(restoreStageView({ boot_stage: "launch_deadline_elapsed", boot_failure_reason: "still connecting" }).phase, "restoring");
console.log("restore stages: sleeping, requested, backend, session, connection, readiness, prompt and failure passed");
