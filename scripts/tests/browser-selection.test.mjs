import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("../../src-tauri/crates/domios-surface/src/surface/browser/selection.js", import.meta.url), "utf8");
const read = (activeElement, selected = "") => vm.runInNewContext(source, { document: { activeElement }, window: { getSelection: () => selected } });

test("copy reads selected page text and selected input ranges", () => {
  assert.equal(read(null, "chosen page text"), "chosen page text");
  assert.equal(read({ tagName: "INPUT", type: "text", value: "hello world", selectionStart: 6, selectionEnd: 11 }), "world");
  assert.equal(read({ tagName: "TEXTAREA", value: "line1\nline2", selectionStart: 0, selectionEnd: 5 }), "line1");
});
test("empty selections do not copy a whole field or password", () => {
  assert.equal(read(null), "");
  assert.equal(read({ tagName: "INPUT", type: "password", value: "private", selectionStart: 0, selectionEnd: 7 }), "");
  assert.equal(read({ tagName: "INPUT", type: "text", value: "unselected", selectionStart: 0, selectionEnd: 0 }), "");
});
test("oversized selections are refused rather than silently truncated", () => {
  assert.throws(() => read(null, "x".repeat(1_048_577)), /clipboard limit/);
});
