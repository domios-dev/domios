import assert from "node:assert/strict";
import { tabActivity } from "./tabActivity";
import { restoredBoundAgent } from "./tabActivity.restore.fixture";
import type { BootStage } from "./types";

for (const boot_stage of ["pty_starting", "pty_spawned", "reader_ready", "shell_ready"] satisfies BootStage[]) {
  for (const [state, expected] of [["working", "working"], ["idle", "idle"], ["question", "waiting-on-user"]] as const) {
    const activity = tabActivity({ ...restoredBoundAgent, boot_stage, agent_state: { state } });
    assert.equal(activity.kind, expected);
    assert.equal(activity.launching, false);
    assert.equal(activity.canStop, state === "working");
  }
}

for (const patch of [
  { agent_session_id: null }, { agent_session_id: " " }, { pty_id: null },
  { session_binding_nonce: null }, { reported_session_binding_nonce: null },
  { reported_session_binding_nonce: "another-launch" },
  { agent_booting: true }, { agent_booting: undefined },
  { boot_stage: "launch_written" as const }, { boot_stage: "delivering_instructions" as const },
]) {
  assert.equal(tabActivity({ ...restoredBoundAgent, ...patch }, "Working").kind, "starting", JSON.stringify(patch));
}
assert.equal(tabActivity({ ...restoredBoundAgent, frozen: true }).kind, "idle");
assert.equal(tabActivity({ ...restoredBoundAgent, boot_stage: "spawn_failed" }).kind, "failed");
assert.equal(tabActivity({ ...restoredBoundAgent, pending_prompt: { question: "opaque", options: [] } }).kind, "waiting-on-user");
console.log("restored activity: 12 own-session states, 10 incomplete/foreign-launch proofs, and precedence checks passed");
