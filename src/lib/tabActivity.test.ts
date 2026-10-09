import assert from "node:assert/strict";
import { tabActivity, type TabActivityInput, type TabActivity } from "./tabActivity";
import type { TabStatus } from "./types";

const agent: TabActivityInput = { tab_type: "Terminal", agent_type: "fixture" };
const cases: Array<[string, TabActivityInput | null, TabStatus | null, TabActivity["kind"], boolean, boolean]> = [
  ["unknown", null, null, "idle", false, false],
  ["tracker working without classifier", agent, "Working", "working", true, true],
  ["tracker working overrides stale idle", { ...agent, agent_state: { state: "idle" } }, "Working", "working", true, true],
  ["tracker idle overrides stale working", { ...agent, agent_state: { state: "working" } }, "Idle", "idle", false, false],
  ["tracker waiting", agent, "Waiting", "waiting-on-user", false, false],
  ["provider working without tracker", { ...agent, agent_state: { state: "working" } }, null, "working", true, true],
  ["provider working during launch", { ...agent, boot_stage: "launch_written", agent_state: { state: "working" } }, null, "working", true, true],
  ["provider question", { ...agent, agent_state: { state: "question" } }, null, "waiting-on-user", false, false],
  ["booting", { ...agent, agent_booting: true }, "Idle", "starting", true, false],
  ["launch written", { ...agent, boot_stage: "launch_written" }, null, "starting", true, false],
  ["ready stage stays idle", { ...agent, boot_stage: "agent_ready" }, "Idle", "idle", false, false],
  ["reattached stage stays idle", { ...agent, boot_stage: "pty_reattached" }, null, "idle", false, false],
  ["owner prompt", { ...agent, boot_stage: "awaiting_owner_prompt" }, "Working", "waiting-on-user", false, false],
  ["spawn failed", { ...agent, boot_stage: "spawn_failed" }, "Working", "failed", false, false],
  ["dead outranks prompt", { ...agent, boot_stage: "awaiting_owner_prompt" }, "Dead", "failed", false, false],
  ["tracker error", agent, "Error", "failed", false, false],
  ["shell running", { tab_type: "Terminal", shell_command_state: { activity: "running", last_exit_code: null } }, "Idle", "working", true, true],
  ["shell idle ignores output churn", { tab_type: "Terminal", shell_command_state: { activity: "idle", last_exit_code: 0 } }, "Working", "idle", false, false],
];

for (const [name, tab, status, kind, active, canStop] of cases) {
  const result = tabActivity(tab, status);
  assert.equal(result.kind, kind, name);
  assert.equal(result.active, active, name);
  assert.equal(result.canStop, canStop, name);
}

assert.equal(tabActivity({ ...agent, agent_state: { state: "question", subtype: "permission" } }).label, "Permission needed");
assert.equal(tabActivity({ ...agent, pending_prompt: { question: "Opaque question?", options: [] } }, "Waiting").label, "Opaque question?");
assert.equal(tabActivity({ ...agent, agent_state: { state: "idle", status_line: "Stale", phase: "thinking" } }, "Working").label, "Working");
assert.equal(tabActivity({ ...agent, agent_state: { state: "idle", phase: "thinking" } }, "Working").phase, "work");
assert.equal(tabActivity({ ...agent, agent_state: { state: "working", phase: "streaming", status_line: " Writing " } }).label, "Writing");
assert.equal(tabActivity({ ...agent, agent_state: { state: "working", phase: "streaming" } }).phase, "streaming");
console.log(`tab activity: ${cases.length} table cases passed`);
