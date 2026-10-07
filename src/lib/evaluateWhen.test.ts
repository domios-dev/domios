import assert from "node:assert/strict";
import { test } from "node:test";
import { evaluateWhen } from "./evaluateWhen";

test("empty clauses are unrestricted; known and absent keys use boolean truth", () => {
  assert.equal(evaluateWhen(undefined, {}), true);
  assert.equal(evaluateWhen("focused", { focused: true }), true);
  assert.equal(evaluateWhen("focused", {}), false);
  assert.equal(evaluateWhen("!focused", { focused: false }), true);
});
test("logical precedence and parentheses gate independent composer contexts", () => {
  const context = { focused: true, composing: false, pickerOpen: true };
  assert.equal(evaluateWhen("focused && !composing && !pickerOpen", context), false);
  assert.equal(evaluateWhen("focused && (!composing || !pickerOpen)", context), true);
  assert.equal(evaluateWhen("false || true && false", {}), false);
});
test("equality supports quoted and bare literals without coercion", () => {
  assert.equal(evaluateWhen("mode == floating", { mode: "floating" }), true);
  assert.equal(evaluateWhen('mode != "docked"', { mode: "floating" }), true);
  assert.equal(evaluateWhen("count === 2", { count: 2 }), true);
  assert.equal(evaluateWhen('count == "2"', { count: 2 }), false);
  assert.equal(evaluateWhen("missing != x", {}), false);
});
test("malformed or executable clauses fail closed even behind true branches", () => {
  for (const expr of ["true || (", "focused &&", "()", "a + b", "globalThis.run()", "a ==", "a b", "true || unknown()", "(".repeat(40) + "true" + ")".repeat(40)]) assert.equal(evaluateWhen(expr, { focused: true }), false, expr);
  assert.equal(evaluateWhen("constructor", {}), false);
});
