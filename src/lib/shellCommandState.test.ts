import assert from "node:assert/strict";
import { applyShellState, isUtilityShell, shellPurposeLabel, shellStateLabel } from "./shellCommandState";
import type { Tab } from "./types";

const shell: Tab = { id: "shell", title: "Build", tab_type: "Terminal", pty_id: "pty", cwd: null };
assert.equal(shellStateLabel(), "unknown");
assert.equal(shellStateLabel({ activity: "unknown", last_exit_code: null }), "unknown");
assert.equal(shellStateLabel({ activity: "running", last_exit_code: 9 }), "running");
assert.equal(shellStateLabel({ activity: "idle", last_exit_code: null }), "idle");
assert.equal(shellStateLabel({ activity: "idle", last_exit_code: 0 }), "idle, last exit ok");
assert.equal(shellStateLabel({ activity: "idle", last_exit_code: 9 }), "idle, last exit failed (9)");
assert.equal(shellPurposeLabel("build"), "build shell");
assert.equal(shellPurposeLabel(), "shell");
const running = { activity: "running", last_exit_code: null } as const;
assert.equal(applyShellState(shell, "old-pty", running), shell);
assert.deepEqual(applyShellState(shell, "pty", running).shell_command_state, running);
for (const overrides of [
  { agent_type: "opaque" }, { role: "Worker" as const }, { roleless_tracked: true },
  { backing: { kind: "chat_backend" as const, plugin_id: "plugin", session_id: "session" } },
  { backing: { kind: "surface" as const, plugin_id: "plugin", surface_id: "board", instance_id: "i" } },
  { tab_type: "Choice" as const },
]) {
  const tab = { ...shell, ...overrides };
  assert.equal(isUtilityShell(tab), false);
  assert.equal(applyShellState(tab, "pty", running), tab);
}
console.log("shellCommandState: passed");
