import { composerVisible } from "./chatBodyVisibility";
import assert from "node:assert/strict";
import { test } from "node:test";
import { chatSetupView, heldMessagesForLaunch } from "./chatSetupModel";

test("an observed spawn failure replaces loading even after a transcript attached", () => {
  const failed = { boot_stage: "spawn_failed" as const, boot_failure_reason: "opaque-refusal" };
  for (const attached of [false, true]) {
    assert.deepEqual(chatSetupView(failed, attached), { kind: "launch_failed", reason: "opaque-refusal" });
  }
});

test("a pending runtime deadline keeps startup until a transcript attaches or runtime recovers", () => {
  const pending = { boot_stage: "agent_ready" as const, session_bound: true,
    agent_runtime_state: { state: "degraded" as const, epoch: 2, reason: "Deadline: opaque" } };
  assert.equal(chatSetupView(pending, false).kind, "starting");
  assert.equal(chatSetupView(pending, true).kind, "none");
  const recovered = { ...pending, agent_runtime_state: { state: "loaded" as const, epoch: 3 } };
  assert.equal(chatSetupView(recovered, true).kind, "none");
  assert.equal(chatSetupView(recovered, false).kind, "none");
});

test("only undismissed backend-held messages are shown as waiting for launch", () => {
  assert.equal(heldMessagesForLaunch([{ status: { state: "held" } }, { status: { state: "written" } },
    { status: { state: "held" }, dismissed: true }, {}, { status: { state: "held" } }]), 2);
});

test("composer follows the launch-failure body state and returns on relaunch", () => {
  for (const attached of [false, true]) {
    assert.equal(composerVisible(chatSetupView({ boot_stage: "spawn_failed" }, attached)), false);
    assert.equal(composerVisible(chatSetupView({ boot_stage: "pty_starting" }, attached)), true);
    assert.equal(composerVisible(chatSetupView({ boot_stage: "agent_ready" }, attached)), true);
  }
  assert.equal(composerVisible({ kind: "failed", reason: "no_transcript_source" }), true);
});
