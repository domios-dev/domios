import assert from "node:assert/strict";
import { test } from "node:test";
import { boundedMessageIsClipped, boundedMessageLayout } from "./boundedMessageModel";

test("expanded content has no height cap and supports horizontal scrolling", () => {
  assert.deepEqual(boundedMessageLayout("500px", false), { maxHeight: "500px", overflowY: "hidden", overflowX: "auto" });
  assert.deepEqual(boundedMessageLayout("500px", true), { maxHeight: "none", overflowY: "auto", overflowX: "auto" });
  assert.deepEqual(boundedMessageLayout("16rem", true), { maxHeight: "none", overflowY: "auto", overflowX: "auto" });
});

test("short or hidden bodies need no Show all, growing bodies do", () => {
  assert.equal(boundedMessageIsClipped(250, 256), false);
  assert.equal(boundedMessageIsClipped(257, 256), false);
  assert.equal(boundedMessageIsClipped(800, 256), true);
  assert.equal(boundedMessageIsClipped(800, 0), false);
});
