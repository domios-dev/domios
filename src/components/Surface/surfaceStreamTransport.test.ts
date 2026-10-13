// Run: npx tsx src/components/Surface/surfaceStreamTransport.test.ts

import assert from "node:assert/strict";
import { clickInput, keyInput, MODIFIER, wheelInput } from "./streamModel";
import { inputArgs, isPreviewPaused, resizeArgs } from "./surfaceStreamTransport";

assert.deepEqual(resizeArgs({ w: 640, h: 480, dpr: 2 }), { width: 640, height: 480, dpr: 2 });

assert.deepEqual(inputArgs(clickInput({ x: 10, y: 20 }, 2)), { kind: "click", x: 10, y: 20, button: "left", clickCount: 2 }, "click preserves its count");
assert.deepEqual(
  inputArgs(wheelInput({ x: 1, y: 2 }, { deltaX: 5, deltaY: 3, deltaMode: 1 }, 600)),
  { kind: "wheel", x: 1, y: 2, deltaX: 80, deltaY: 48 },
  "wheel preserves both pixel axes",
);
assert.deepEqual(inputArgs({ kind: "text", text: "a" }), { kind: "text", text: "a" });

const enter = keyInput({ key: "Enter", code: "Enter", altKey: false, ctrlKey: true, metaKey: false, shiftKey: true });
assert.ok(enter);
assert.deepEqual(inputArgs(enter), { ...enter }, "key preserves code, text and modifiers");
assert.deepEqual(
  inputArgs({ kind: "key", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9, modifiers: MODIFIER.alt | MODIFIER.meta }),
  { kind: "key", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9, modifiers: MODIFIER.alt | MODIFIER.meta },
);

assert.ok(isPreviewPaused({ text: "Preview paused", kind: "preview_paused" }));
assert.ok(!isPreviewPaused({ text: "boom", tone: "error" }), "an untyped notice is not a paused preview");
assert.ok(!isPreviewPaused(null));

console.log("✓ surfaceStreamTransport tests green");
