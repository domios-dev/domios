import assert from "node:assert/strict";
import test from "node:test";
import { canvasPanDecision } from "./canvasPanModel";

test("canvas pan gestures take precedence only for the intended pointer and focus", () => {
  assert.equal(canvasPanDecision({ button: 0, spaceHeld: true, overItem: true, typingFocus: false }), "pan", "Space pans over draggable or selected items");
  assert.equal(canvasPanDecision({ button: 0, spaceHeld: true, overItem: false, typingFocus: true }), "pass", "Space keeps its meaning in a focused note or HTML frame");
  assert.equal(canvasPanDecision({ button: 1, spaceHeld: false, overItem: true, typingFocus: true }), "pan", "middle button pans over items regardless of focus");
  assert.equal(canvasPanDecision({ button: 0, spaceHeld: false, overItem: false, typingFocus: false }), "pan", "primary drag pans on empty board area");
  assert.equal(canvasPanDecision({ button: 0, spaceHeld: false, overItem: true, typingFocus: false }), "pass", "primary drag on an item remains an item gesture");
  assert.equal(canvasPanDecision({ button: 2, spaceHeld: true, overItem: false, typingFocus: false }), "pass");
});
