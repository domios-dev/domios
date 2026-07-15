import assert from "node:assert/strict";
import test from "node:test";
import { clampPortalCenter } from "./portalPosition";

test("keeps a centered portal inside the viewport", () => {
  assert.equal(clampPortalCenter(10, 260, 390), 138);
  assert.equal(clampPortalCenter(380, 260, 390), 252);
  assert.equal(clampPortalCenter(195, 260, 390), 195);
});

test("handles portals wider than a narrow viewport", () => {
  assert.equal(clampPortalCenter(12, 320, 280), 140);
});
