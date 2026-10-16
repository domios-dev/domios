import assert from "node:assert/strict";
import { test } from "node:test";
import { chatOverflowAnchorFor } from "./chatAnchorAuthority";

test("ownership selects a single anchoring authority", () => {
  for (const mode of ["pinned", "detached"] as const) {
    assert.equal(chatOverflowAnchorFor(mode, true), "none");
  }
  assert.equal(chatOverflowAnchorFor("detached", false), "auto");
  assert.equal(chatOverflowAnchorFor("pinned", false), "none");
});
