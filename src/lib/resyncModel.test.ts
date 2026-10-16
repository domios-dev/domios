import assert from "node:assert/strict";
import test from "node:test";
import { replayCursorForTarget, resyncRequiresTranscriptRecovery } from "./resyncModel";

test("valid reconnect and idle wake preserve the transcript projection", () => {
  for (let count = 0; count < 100; count++) {
    assert.equal(resyncRequiresTranscriptRecovery("reconnected"), false);
    assert.equal(resyncRequiresTranscriptRecovery("wake"), false);
  }
});

test("event loss and daemon epoch changes still require transcript recovery", () => {
  assert.equal(resyncRequiresTranscriptRecovery("overflow"), true);
  assert.equal(resyncRequiresTranscriptRecovery("epoch_changed"), true);
});

test("an observed reconnect can retry a failed initial attachment", () => {
  assert.equal(resyncRequiresTranscriptRecovery("reconnected", true), true);
  assert.equal(resyncRequiresTranscriptRecovery("wake", true), true);
});

test("same-target reconnect retains an in-memory cursor when storage is unavailable", () => {
  assert.equal(replayCursorForTarget("dev", "boot:42", "dev", null), "boot:42");
  assert.equal(replayCursorForTarget("dev", "boot:42", "dev", "boot:39"), "boot:42");
});

test("peer switches use only the destination cursor and cold pages use saved replay", () => {
  assert.equal(replayCursorForTarget("dev", "boot:42", "peer", null), null);
  assert.equal(replayCursorForTarget("dev", "boot:42", "peer", "peer:9"), "peer:9");
  assert.equal(replayCursorForTarget("dev", null, "dev", "boot:39"), "boot:39");
});
