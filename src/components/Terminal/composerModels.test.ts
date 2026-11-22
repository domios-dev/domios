import assert from "node:assert/strict";
import { test } from "node:test";
import { composerPickerState, createApprovalLatch, recallPrompt, approvalActions, approvalHotkey, contextMeter, rememberPrompt, swapStash } from "./composerModels";
import { modelSwitchLabel, supportsLiveModelSwitch } from "../../lib/modelSwitch";

test("approval docking orders allow, always allow, then reject without changing response values or hotkeys", () => {
  const prompt = { question: "opaque", options: [{ label: "C", value: "reject" }, { label: "A", value: "once" }, { label: "B", value: "always" }] };
  assert.deepEqual(approvalActions(prompt).map(({ value, hotkey }) => [value, hotkey]), [["once", 2], ["always", 3], ["reject", 1]]);
  assert.equal(approvalHotkey(prompt, "Digit1"), "reject");
  assert.equal(approvalHotkey(prompt, "Digit9"), undefined);
  assert.equal(approvalHotkey(prompt, "Numpad1"), undefined);
  assert.equal(approvalHotkey({ ...prompt, multi_select: true }, "Digit1"), undefined);
});

test("context requires real finite token counts and caps only the display percentage", () => {
  for (const value of [null, {}, { used_tokens: 3 }, { used_tokens: -1, window_tokens: 10 }, { used_tokens: 4, window_tokens: 0 }, { used_tokens: Infinity, window_tokens: 10 }]) assert.equal(contextMeter(value), null);
  assert.deepEqual(contextMeter({ used_tokens: 25, window_tokens: 100 }), { used: 25, window: 100, percent: 25 });
  assert.deepEqual(contextMeter({ used_tokens: 120, window_tokens: 100 }), { used: 120, window: 100, percent: 100 });
});

test("recall retains distinct submitted prompts within a bounded stack", () => {
  assert.deepEqual(rememberPrompt(["a"], "a"), ["a"]);
  assert.deepEqual(rememberPrompt(["a"], " "), ["a"]);
  assert.deepEqual(rememberPrompt(["a"], "b"), ["a", "b"]);
  assert.equal(rememberPrompt(Array.from({ length: 50 }, (_, i) => String(i)), "next").length, 50);
});

test("stash swaps without discarding an existing draft", () => {
  const first = swapStash("draft", "");
  assert.deepEqual(first, { draft: "", stashed: "draft" });
  assert.deepEqual(swapStash("new", first.stashed), { draft: "draft", stashed: "new" });
});

test("model switching remains unavailable without a declaration", () => {
  assert.equal(supportsLiveModelSwitch(undefined), false);
  assert.equal(modelSwitchLabel("next_turn"), "Next turn");
  assert.equal(modelSwitchLabel(undefined), "Unavailable");
});

test("recall traverses history and restores the draft on ArrowDown", () => {
  const first = recallPrompt(["a", "b"], null, "draft", -1);
  assert.equal(first.text, "b");
  const second = recallPrompt(["a", "b"], first.position, "b", -1);
  assert.equal(second.text, "a");
  const next = recallPrompt(["a", "b"], second.position, "a", 1);
  assert.equal(next.text, "b");
  assert.equal(recallPrompt(["a", "b"], next.position, "b", 1).text, "draft");
});

test("approval latch permits one response across competing UI actions", () => {
  const latch = createApprovalLatch();
  assert.equal(latch.claim(), true);
  assert.equal(latch.claim(), false);
  assert.equal(latch.claim(), false);
  assert.equal(createApprovalLatch().claim(), true);
});

test("picker gates unknown declarations and keeps unimplemented backend controls off", () => {
  assert.deepEqual(composerPickerState(undefined, undefined), { modelEnabled: false, fastEnabled: false, runtimeMode: "unavailable" });
  assert.deepEqual(composerPickerState("unsupported", true), { modelEnabled: false, fastEnabled: false, runtimeMode: "launch_flag" });
  for (const mode of ["live", "restart_resume", "next_turn"]) {
    assert.deepEqual(composerPickerState(mode, false), { modelEnabled: true, fastEnabled: false, runtimeMode: "unavailable" });
  }
  assert.equal(composerPickerState("unknown-protocol", false).modelEnabled, false);
});
