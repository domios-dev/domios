import assert from "node:assert/strict";
import { test } from "node:test";
import { boundedMessageIsClipped, boundedMessageLayout, expandedMessageHeight } from "./boundedMessageModel";

test("preview retains its height and clips, expansion adds thirty percent and permits native scrolling", () => {
  assert.deepEqual(boundedMessageLayout("500px", false), { maxHeight: "500px", overflowY: "hidden" });
  assert.deepEqual(boundedMessageLayout("500px", true), { maxHeight: "650px", overflowY: "auto" });
  assert.deepEqual(boundedMessageLayout("16rem", true), { maxHeight: "20.8rem", overflowY: "auto" });
  assert.equal(expandedMessageHeight(320), 416);
});

test("short or hidden bodies need no Show all, growing bodies do", () => {
  assert.equal(boundedMessageIsClipped(250, 256), false);
  assert.equal(boundedMessageIsClipped(257, 256), false);
  assert.equal(boundedMessageIsClipped(800, 256), true);
  assert.equal(boundedMessageIsClipped(800, 0), false);
});
