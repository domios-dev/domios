import assert from "node:assert/strict";
import { tabActivity, type TabActivityInput, type TabActivity } from "./tabActivity";
import type { BootStage, TabStatus } from "./types";
import { aggregateTabStatuses } from "../components/Sidebar/tabStatuses";
import type { Tab } from "./types";

const agent: TabActivityInput = { tab_type: "Terminal", agent_type: "fixture", agent_session_id: "bound-session" };
const cases: Array<[string, TabActivityInput | null, TabStatus | null, TabActivity["kind"], boolean, boolean]> = [
  ["unknown", null, null, "idle", false, false],
  ["tracker output without classifier", agent, "Working", "idle", false, false],
  ["tracker output with idle classifier", { ...agent, agent_state: { state: "idle" } }, "Working", "idle", false, false],
  ["bound live work overrides stale tracker", { ...agent, agent_state: { state: "working" } }, "Idle", "working", true, true],
  ["tracker waiting", agent, "Waiting", "waiting-on-user", false, false],
  ["bound provider working", { ...agent, agent_state: { state: "working" } }, null, "working", true, true],
  ["unbound provider working", { ...agent, agent_session_id: null, agent_state: { state: "working" } }, "Working", "starting", false, false],
  ["provider work during launch", { ...agent, boot_stage: "launch_written", agent_state: { state: "working" } }, "Working", "starting", false, false],
  ["provider question", { ...agent, agent_state: { state: "question" } }, null, "waiting-on-user", false, false],
  ["agent_ready beats agent_booting", { ...agent, agent_booting: true, boot_stage: "agent_ready" }, "Idle", "idle", false, false],
  ["booting", { ...agent, agent_booting: true }, "Working", "starting", false, false],
  ["launch written", { ...agent, boot_stage: "launch_written" }, null, "starting", false, false],
  ["lazy composer ready before session creation", { ...agent, agent_session_id: null, boot_stage: "agent_ready" }, "Idle", "idle", false, false],
  ["ready stage stays idle", { ...agent, boot_stage: "agent_ready" }, "Idle", "idle", false, false],
  ["reattached stage stays idle", { ...agent, boot_stage: "pty_reattached" }, null, "idle", false, false],
  ["owner prompt", { ...agent, boot_stage: "awaiting_owner_prompt" }, "Working", "waiting-on-user", false, false],
  ["spawn failed", { ...agent, boot_stage: "spawn_failed" }, "Working", "failed", false, false],
  ["dead outranks prompt", { ...agent, boot_stage: "awaiting_owner_prompt" }, "Dead", "failed", false, false],
  ["tracker error", agent, "Error", "failed", false, false],
  ["shell running", { tab_type: "Terminal", shell_command_state: { activity: "running", last_exit_code: null } }, "Idle", "working", true, true],
  ["ready shell command", { tab_type: "Terminal", boot_stage: "shell_ready", shell_command_state: { activity: "running", last_exit_code: null } }, "Working", "working", true, true],
  ["shell idle ignores output churn", { tab_type: "Terminal", shell_command_state: { activity: "idle", last_exit_code: 0 } }, "Working", "idle", false, false],
  ["bound chat backend", { backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "chat-session" }, agent_state: { state: "working" } }, null, "working", true, true],
];

for (const [name, tab, status, kind, active, canStop] of cases) {
  const result = tabActivity(tab, status);
  assert.equal(result.kind, kind, name);
  assert.equal(result.active, active, name);
  assert.equal(result.canStop, canStop, name);
  assert.equal(result.status === "Working", kind === "working", name);
}

const startingStages: BootStage[] = ["pty_starting", "pty_spawned", "reader_ready", "shell_ready", "agent_launching", "launch_written", "provider_ready", "preparing_role_instructions", "delivering_instructions", "launch_deadline_elapsed", "launch_binding_unconfirmed"];
for (const boot_stage of startingStages) {
  const activity = tabActivity({ ...agent, boot_stage, agent_state: { state: "working" } }, "Working");
  assert.equal(activity.kind, "starting", boot_stage);
  assert.equal(activity.active, false, boot_stage);
  assert.equal(activity.status, "Idle", boot_stage);
  assert.equal(activity.canStop, false, boot_stage);
}
const tabs = Object.fromEntries(startingStages.map((boot_stage) => [boot_stage, { ...agent, id: boot_stage, boot_stage } as Tab]));
const statuses = Object.fromEntries(startingStages.map((stage) => [stage, "Working" as const]));
assert.equal(aggregateTabStatuses(startingStages, statuses, tabs), "Idle");
assert.equal(Object.values(tabs).filter((tab) => tabActivity(tab, "Working").active).length, 0);
assert.equal(Object.values(tabs).filter((tab) => tabActivity(tab, "Working").status === "Working").length, 0);

assert.equal(tabActivity({ ...agent, agent_state: { state: "question", subtype: "permission" } }).subtype, "permission");
const pending = { question: "question-token", options: [] };
assert.equal(tabActivity({ ...agent, pending_prompt: pending }, "Waiting").label, pending.question);
assert.equal(tabActivity({ ...agent, agent_state: { state: "idle", phase: "thinking" } }, "Working").phase, "work");
assert.equal(tabActivity({ ...agent, agent_state: { state: "question", phase: "thinking" } }).phase, "work");
assert.equal(tabActivity({ ...agent, boot_stage: "agent_launching", agent_state: { state: "working", phase: "thinking" } }).phase, "work");
assert.equal(tabActivity({ ...agent, agent_state: { state: "working", phase: "thinking" } }).phase, "thinking");
assert.equal(tabActivity({ ...agent, agent_state: { state: "working", phase: "streaming", status_line: " Writing " } }).label, "Writing");
assert.equal(tabActivity({ ...agent, agent_state: { state: "working", phase: "streaming" } }).phase, "streaming");
const compacting = tabActivity({ ...agent, agent_state: { state: "working", phase: "compacting" } });
assert.equal(compacting.kind, "working");
assert.equal(compacting.phase, "work");
assert.notEqual(compacting.label, tabActivity({ ...agent, agent_state: { state: "working" } }).label);
assert.equal(tabActivity({ ...agent, agent_state: { state: "working", phase: "compacting", status_line: "Reading" } }).label, "Reading");
const interrupted = tabActivity({ ...agent, agent_state: { state: "idle", phase: "interrupted" } });
assert.equal(interrupted.kind, "idle");
assert.notEqual(interrupted.label, tabActivity({ ...agent, agent_state: { state: "idle", phase: "failed" } }).label);
assert.equal(tabActivity({ ...agent, agent_state: { state: "question", subtype: "error" } }).kind, "waiting-on-user");
const providerSignals = { auth: null, api_error: "usage_limit" as const, permission_mode: null };
assert.equal(tabActivity({ ...agent, provider_signals: providerSignals }).label, "Usage limit reached");
assert.equal(tabActivity({ ...agent, pending_prompt: pending, provider_signals: providerSignals }, "Waiting").label, pending.question);
assert.equal(tabActivity({ ...agent, agent_state: { state: "working" }, provider_signals: providerSignals }).label, "Working");
console.log(`tab activity: ${cases.length} table cases, ${startingStages.length} boot stages and working-count checks passed`);

assert.equal(tabActivity({ ...agent, frozen: true }, "Working").active, false);
assert.equal(tabActivity({ ...agent, frozen: true }, "Working").launching, false);
