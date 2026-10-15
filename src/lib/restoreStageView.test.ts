import assert from "node:assert/strict";
import { restorePanelOwnsBody, restoreStageView } from "./restoreStageView";
import type { Tab } from "./types";

const tab: Partial<Tab> = { agent_type: "fixture", agent_session_id: "saved", frozen: true,
  boot_stage: "agent_ready", agent_runtime_state: { state: "loaded", epoch: 1 } };
assert.equal(restoreStageView(tab).phase, "sleeping");
const queued = restoreStageView(tab, { requested: true });
assert.equal(queued.phase, "restoring");
assert.equal(queued.current, -1);
assert(queued.steps.every(step => step.state === "pending"));
const awake = { ...tab, frozen: false };
for (const [boot_stage, current] of [["pty_starting", 0], ["reader_ready", 0], ["agent_launching", 1], ["launch_written", 1], ["provider_ready", 2], ["launch_binding_unconfirmed", 2]] as const) {
  const view = restoreStageView({ ...awake, boot_stage });
  assert.equal(view.phase, "restoring");
  assert.equal(view.current, current);
}
assert.equal(restoreStageView(awake).phase, "ready");
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
assert.equal(restoreStageView({ boot_stage: "shell_ready" }).phase, "ready");
assert.equal(restoreStageView({ boot_stage: "pty_spawned" }).phase, "restoring");
assert.equal(restoreStageView({ boot_stage: "launch_deadline_elapsed", boot_failure_reason: "still connecting" }).phase, "restoring");
console.log("restore stages: sleeping, requested, backend, session, connection, readiness, prompt and failure passed");
