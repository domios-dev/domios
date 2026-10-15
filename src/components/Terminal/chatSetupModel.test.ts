import assert from "node:assert/strict";
import { test } from "node:test";
import { chatSetupView, heldMessagesForLaunch } from "./chatSetupModel";

test("an observed spawn failure replaces loading even after a transcript attached", () => {
  const failed = { boot_stage: "spawn_failed" as const, boot_failure_reason: "opaque-refusal" };
  for (const attached of [false, true]) {
    assert.deepEqual(chatSetupView(failed, attached), { kind: "launch_failed", reason: "opaque-refusal" });
  }
});

test("only undismissed backend-held messages are shown as waiting for launch", () => {
  assert.equal(heldMessagesForLaunch([{ status: { state: "held" } }, { status: { state: "written" } },
    { status: { state: "held" }, dismissed: true }, {}, { status: { state: "held" } }]), 2);
});
