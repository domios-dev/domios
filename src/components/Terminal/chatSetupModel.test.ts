import { composerVisible } from "./chatBodyVisibility";
import assert from "node:assert/strict";
import { test } from "node:test";
import { chatSetupView, composerSendPolicy, heldMessagesForLaunch } from "./chatSetupModel";
import type { Tab } from "../../lib/types";

test("an observed spawn failure replaces loading even after a transcript attached", () => {
  const failed = { boot_stage: "spawn_failed" as const, boot_failure_reason: "opaque-refusal" };
  for (const attached of [false, true]) {
    assert.deepEqual(chatSetupView(failed, attached), { kind: "launch_failed", reason: "opaque-refusal" });
  }
});

test("a launched session whose relaunch failed never takes the body from its transcript", () => {
  const launched = { boot_stage: "spawn_failed" as const, boot_failure_reason: "opaque-refusal", agent_session_id: "opaque-session",
    backing: { kind: "chat_backend" as const, plugin_id: "opaque", session_id: "opaque-session" },
    agent_runtime_state: { state: "degraded" as const, epoch: 2, failureCode: "provider_refused" as const, reason: "opaque-refusal" } };
  for (const attached of [false, true]) {
    assert.deepEqual(chatSetupView(launched, attached), { kind: "none" });
    assert.equal(composerVisible(chatSetupView(launched, attached)), true);
    assert.deepEqual(chatSetupView({ boot_stage: "spawn_failed" as const, agent_session_id: "opaque-session" }, attached), { kind: "none" });
  }
});

test("a chat launch that failed before any session reports failure instead of loading", () => {
  const degraded = { state: "degraded" as const, epoch: 2, failureCode: "provider_refused" as const, reason: "opaque-refusal" };
  const unbound = { backing: { kind: "chat_backend" as const, plugin_id: "opaque", session_id: "" }, agent_runtime_state: degraded };
  assert.deepEqual(chatSetupView(unbound), { kind: "launch_failed", reason: "opaque-refusal" });
  assert.equal(chatSetupView({ ...unbound, agent_booting: true }).kind, "starting");
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

test("the exact tab accepts composer sends throughout its lifecycle", () => {
  const retired = [{ nonce: "retired", session_id: "own-session" }];
  const states = {
    launching: { agent_type: "fixture", boot_stage: "agent_launching" },
    restoring: { agent_type: "fixture", frozen: true, boot_stage: "pty_starting" },
    exited: { backing: { kind: "pty" }, boot_stage: "shell_ready", prior_session_binding_nonces: retired },
    restarting: { backing: { kind: "pty" }, boot_stage: "pty_starting", prior_session_binding_nonces: retired },
    ready: { agent_type: "fixture", boot_stage: "agent_ready" },
    failed: { agent_type: "fixture", boot_stage: "spawn_failed" },
  } satisfies Record<string, Partial<Tab>>;
  for (const [state, tab] of Object.entries(states)) {
    assert.equal(composerSendPolicy(tab).allowed, true, state);
    assert.equal(composerSendPolicy(tab).terminalFallback, state === "ready", state);
    assert.equal(composerVisible(chatSetupView(tab)), true, state);
  }
  assert.equal(composerSendPolicy(undefined).allowed, false);
  assert.equal(composerVisible({ kind: "locked" }), false);
  assert.deepEqual(chatSetupView(states.exited, true), { kind: "exited" });
  assert.deepEqual(chatSetupView({ ...states.exited, boot_stage: "pty_reattached" }), { kind: "exited" });
  assert.equal(chatSetupView(states.restarting).kind, "starting");
  assert.notEqual(chatSetupView({ backing: { kind: "pty" }, boot_stage: "shell_ready" }).kind, "exited");
});
