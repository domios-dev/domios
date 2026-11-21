import { strict as assert } from "node:assert";
import { test } from "node:test";
import { DictationSessionModel, type DictationUpdate } from "./dictationSessionModel";

const update = (sequence: number, provisional: string, fields: Partial<DictationUpdate> = {}): DictationUpdate =>
  ({ session: "one", sequence, provisional, committed: "", final: false, ...fields });

test("revises the exact selection and only persists a final or committed segment", () => {
  const draft = new DictationSessionModel("one", "prefix replace suffix", 7, 14);
  draft.update(update(1, "first"));
  assert.equal(draft.text, "prefix first suffix");
  assert.equal(draft.durable, "prefix replace suffix");
  draft.update(update(2, "first revision"));
  assert.equal(draft.text, "prefix first revision suffix");
  draft.update(update(3, "", { committed: "final words", final: true }));
  assert.equal(draft.durable, "prefix final words suffix");
  assert.equal(draft.update(update(3, "", { committed: "duplicate", final: true })), null);
  assert.equal(draft.durable, "prefix final words suffix");
});

test("cancel restores the captured selection and retains typed edits and sealed speech", () => {
  const draft = new DictationSessionModel("one", "before selected after", 7, 15);
  draft.update(update(1, "pending", { committed: "sealed" }));
  assert.equal(draft.text, "before sealed pending after");
  assert.equal(draft.durable, "before sealed selected after");
  draft.edit("typed before sealed pending after");
  draft.edit("typed before sealed pending after plus");
  draft.cancel();
  assert.equal(draft.text, "typed before sealed selected after plus");
  assert.equal(draft.update(update(2, "late", { final: true })), null);
});

test("a final silence commits a prior segment without restoring the replaced selection", () => {
  const draft = new DictationSessionModel("one", "before chosen after", 7, 13);
  draft.update(update(1, "", { committed: "spoken" }));
  draft.update(update(2, ""));
  draft.update(update(3, "", { final: true }));
  assert.equal(draft.durable, "before spoken after");
});

test("repeated words use the exact user edit target and foreign, missing and stale frames are discarded", () => {
  const draft = new DictationSessionModel("one", "echo ", 0, 0);
  draft.update(update(1, "echo"));
  assert.equal(draft.text, "echo echo ");
  draft.edit("echo ", { start: 10, end: 10, inputType: "deleteContentBackward" });
  assert.equal(draft.update(update(3, "gap")), null);
  assert.equal(draft.update(update(2, "foreign", { session: "other", final: true })), null);
  draft.update(update(2, "revised"));
  assert.equal(draft.text, "revised");
  draft.cancel();
  assert.equal(draft.durable, "");
});

test("typing into provisional text keeps the user's new text and subsequent updates stay local", () => {
  const draft = new DictationSessionModel("one", "keep selected tail", 5, 13);
  draft.update(update(1, "preview"));
  draft.edit("keep pretypedview tail", { start: 8, end: 8, inputType: "insertText" });
  assert.equal(draft.durable, "keep selectedtyped tail");
  draft.update(update(2, "next"));
  assert.equal(draft.text, "keep selectedtyped next tail");
  draft.cancel();
  assert.equal(draft.durable, "keep selectedtyped tail");
});

test("replacing the whole draft does not restore text the user chose to remove", () => {
  const draft = new DictationSessionModel("one", "keep selected tail", 5, 13);
  draft.update(update(1, "preview"));
  draft.edit("replacement", { start: 0, end: draft.text.length, inputType: "insertText" });
  draft.update(update(2, "next"));
  draft.cancel();
  assert.equal(draft.durable, "replacement");
});
