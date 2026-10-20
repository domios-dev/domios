import assert from "node:assert/strict";
import { test } from "node:test";
import { jumpTapAccepted } from "./chatJumpIntent";

test("a tap must begin while idle", () => {
  assert.equal(jumpTapAccepted(false), false);
  assert.equal(jumpTapAccepted(true), true);
});
