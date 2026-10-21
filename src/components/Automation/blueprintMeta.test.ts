import assert from "node:assert/strict";
import { test } from "node:test";
import { blueprintMeta } from "./blueprintMeta";

test("stopped Blueprint metadata includes the note and remaining cleanup time", () => {
  assert.equal(blueprintMeta("Watches releases", "stopped", 1_801_000, 1_000), "Watches releases · deletes in 30 min");
  assert.equal(blueprintMeta("Watches releases", "stopped", 1_801_000, 1_800_999), "Watches releases · deletes in 1 min");
  assert.equal(blueprintMeta("", "stopped", 1_801_000, 2_000_000), "deletes in 0 min");
  assert.equal(blueprintMeta("Watches releases", "active", null, 2_000_000), "Watches releases");
});
