// Run: npx tsx src/components/Surface/streamModel.test.ts

import assert from "node:assert/strict";
import {
  backingSize,
  canvasToPage,
  clampToDraw,
  clickInput,
  decideKey,
  decodeBase64,
  frameMatchesViewport,
  keyInput,
  layoutStream,
  letterboxRect,
  MODIFIER,
  modifierMask,
  requestedFrameSize,
  sameViewport,
  stageRectToPage,
  viewportAction,
  viewportRequest,
  wheelInput,
  type KeyContext,
  type KeyDecision,
  type KeyLike,
} from "./streamModel";

const NO_MODIFIERS = { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };
const key = (code: string, keyValue: string, modifiers: Partial<typeof NO_MODIFIERS> = {}): KeyLike => ({
  code,
  key: keyValue,
  ...NO_MODIFIERS,
  ...modifiers,
});

const letterboxCases: Array<[string, { width: number; height: number }, { width: number; height: number }, ReturnType<typeof letterboxRect>]> = [
  ["same aspect fills", { width: 800, height: 600 }, { width: 400, height: 300 }, { x: 0, y: 0, width: 400, height: 300 }],
  ["wide frame bars top and bottom", { width: 1600, height: 600 }, { width: 800, height: 600 }, { x: 0, y: 150, width: 800, height: 300 }],
  ["tall frame bars left and right", { width: 600, height: 1200 }, { width: 800, height: 600 }, { x: 250, y: 0, width: 300, height: 600 }],
  ["upscales a small frame", { width: 100, height: 50 }, { width: 400, height: 400 }, { x: 0, y: 100, width: 400, height: 200 }],
  ["empty frame draws nothing", { width: 0, height: 600 }, { width: 800, height: 600 }, { x: 0, y: 0, width: 0, height: 0 }],
  ["empty canvas draws nothing", { width: 800, height: 600 }, { width: 0, height: 0 }, { x: 0, y: 0, width: 0, height: 0 }],
];
for (const [name, frame, canvas, expected] of letterboxCases) {
  assert.deepEqual(letterboxRect(frame, canvas), expected, name);
}

assert.deepEqual(backingSize({ width: 400.4, height: 300 }, 2), { width: 801, height: 600 }, "backing rounds css times dpr");
assert.deepEqual(backingSize({ width: 400, height: 300 }, 0), { width: 400, height: 300 }, "invalid dpr falls back to 1");
assert.deepEqual(backingSize({ width: 100, height: 100 }, 9), { width: 400, height: 400 }, "dpr is capped");

const hiDpi = layoutStream({ width: 1600, height: 600 }, { width: 400, height: 300 }, 2);
assert.deepEqual(hiDpi.backing, { width: 800, height: 600 });
assert.deepEqual(hiDpi.draw, { x: 0, y: 150, width: 800, height: 300 }, "draw rect is in backing pixels");
assert.deepEqual(hiDpi.drawCss, { x: 0, y: 75, width: 400, height: 150 }, "css draw rect divides out the dpr");

const canvasRect = { left: 10, top: 20, width: 400, height: 300 };
const meta = { deviceWidth: 1280, deviceHeight: 960, offsetTop: 0, pageScaleFactor: 1 };
const mappingCases: Array<[string, number, number, ReturnType<typeof letterboxRect>, typeof meta | null, { x: number; y: number } | null]> = [
  ["top-left corner", 10, 20, { x: 0, y: 0, width: 400, height: 300 }, meta, { x: 0, y: 0 }],
  ["centre scales to device size", 210, 170, { x: 0, y: 0, width: 400, height: 300 }, meta, { x: 640, y: 480 }],
  ["letterbox offset subtracts", 60, 20, { x: 50, y: 0, width: 300, height: 300 }, { ...meta, deviceWidth: 600, deviceHeight: 600 }, { x: 0, y: 0 }],
  ["click on a bar is ignored", 30, 20, { x: 50, y: 0, width: 300, height: 300 }, meta, null],
  ["offsetTop is removed", 10, 170, { x: 0, y: 0, width: 400, height: 300 }, { ...meta, offsetTop: 80 }, { x: 0, y: 400 }],
  ["page scale divides", 210, 170, { x: 0, y: 0, width: 400, height: 300 }, { ...meta, pageScaleFactor: 2 }, { x: 320, y: 240 }],
  ["missing metadata maps 1:1", 110, 70, { x: 0, y: 0, width: 400, height: 300 }, null, { x: 100, y: 50 }],
  ["a non-finite pointer yields nothing", Number.NaN, 70, { x: 0, y: 0, width: 400, height: 300 }, meta, null],
  ["zero draw rect yields nothing", 110, 70, { x: 0, y: 0, width: 0, height: 0 }, meta, null],
];
for (const [name, x, y, drawRect, frameMeta, expected] of mappingCases) {
  assert.deepEqual(canvasToPage(x, y, canvasRect, drawRect, frameMeta), expected, name);
}

const fullDraw = { x: 0, y: 0, width: 400, height: 300 };
const snipCases: Array<[string, { x: number; y: number }, { x: number; y: number }, ReturnType<typeof letterboxRect>, Parameters<typeof stageRectToPage>[3], ReturnType<typeof stageRectToPage>]> = [
  ["canvas px scale up to page css px", { x: 100, y: 75 }, { x: 200, y: 150 }, fullDraw, meta, { x: 320, y: 240, width: 320, height: 240 }],
  ["a drag in any direction normalises", { x: 200, y: 150 }, { x: 100, y: 75 }, fullDraw, meta, { x: 320, y: 240, width: 320, height: 240 }],
  ["scroll offset moves the area into document coords", { x: 100, y: 75 }, { x: 200, y: 150 }, fullDraw, { ...meta, scrollOffsetX: 12, scrollOffsetY: 640 }, { x: 332, y: 880, width: 320, height: 240 }],
  ["page scale divides before scroll adds", { x: 0, y: 0 }, { x: 200, y: 150 }, fullDraw, { ...meta, pageScaleFactor: 2, scrollOffsetY: 100 }, { x: 0, y: 100, width: 320, height: 240 }],
  ["offsetTop is removed and never goes negative", { x: 0, y: 0 }, { x: 400, y: 150 }, fullDraw, { ...meta, offsetTop: 80 }, { x: 0, y: 0, width: 1280, height: 400 }],
  ["the letterbox offset subtracts and drags clamp to the frame", { x: 0, y: -40 }, { x: 500, y: 400 }, { x: 50, y: 0, width: 300, height: 300 }, { ...meta, deviceWidth: 600, deviceHeight: 600 }, { x: 0, y: 0, width: 600, height: 600 }],
  ["missing metadata maps 1:1", { x: 10.4, y: 20.6 }, { x: 110, y: 70 }, fullDraw, null, { x: 10, y: 21, width: 100, height: 49 }],
  ["zero draw rect yields nothing", { x: 0, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 0, width: 0, height: 0 }, meta, null],
];
for (const [name, start, end, drawRect, frameMeta, expected] of snipCases) {
  assert.deepEqual(stageRectToPage(start, end, drawRect, frameMeta), expected, name);
}
assert.deepEqual(clampToDraw({ x: -5, y: 400 }, { x: 50, y: 10, width: 300, height: 280 }), { x: 50, y: 290 });

assert.deepEqual(viewportRequest({ width: 640.4, height: 480.6 }, 1.5), { w: 640, h: 481, dpr: 1.5 });
assert.equal(viewportRequest({ width: 0, height: 480 }, 1), null, "a collapsed container requests no stream");
assert.deepEqual(requestedFrameSize({ w: 640, h: 480, dpr: 2 }), { width: 1280, height: 960 });
assert.ok(sameViewport({ w: 1, h: 2, dpr: 1 }, { w: 1, h: 2, dpr: 1 }));
assert.ok(!sameViewport({ w: 1, h: 2, dpr: 1 }, { w: 1, h: 2, dpr: 2 }));
assert.ok(!sameViewport(null, { w: 1, h: 2, dpr: 1 }));
assert.ok(sameViewport(null, null));

const frameCases: Array<[string, Parameters<typeof frameMatchesViewport>[0], boolean]> = [
  ["exact size matches", { deviceWidth: 640, deviceHeight: 480 }, true],
  ["a 1px rounding difference matches", { deviceWidth: 641, deviceHeight: 479 }, true],
  ["another client's width mismatches", { deviceWidth: 1280, deviceHeight: 480 }, false],
  ["another client's height mismatches", { deviceWidth: 640, deviceHeight: 482 }, false],
  ["missing metadata cannot mismatch", null, true],
  ["a zero device size cannot mismatch", { deviceWidth: 0, deviceHeight: 0 }, true],
];
for (const [name, frameMeta, expected] of frameCases) {
  assert.equal(frameMatchesViewport(frameMeta, { w: 640, h: 480, dpr: 2 }), expected, name);
}

const vp = { w: 640, h: 480, dpr: 2 };
const viewportCases: Array<[string, boolean, typeof vp | null, typeof vp | null, ReturnType<typeof viewportAction>]> = [
  ["unmeasured does nothing", false, null, null, "none"],
  ["first size connects with it", false, null, vp, "connect"],
  ["a reconnect after teardown connects again", false, vp, vp, "connect"],
  ["an unchanged size sends nothing", true, vp, { ...vp }, "none"],
  ["a new size resizes", true, vp, { ...vp, w: 641 }, "resize"],
  ["a dpr change resizes", true, vp, { ...vp, dpr: 1 }, "resize"],
];
for (const [name, connected, sent, next, expected] of viewportCases) {
  assert.equal(viewportAction(connected, sent, next), expected, name);
}

assert.deepEqual(MODIFIER, { alt: 1, ctrl: 2, meta: 4, shift: 8 });
const maskCases: Array<[Partial<typeof NO_MODIFIERS>, number]> = [
  [{}, 0],
  [{ altKey: true }, 1],
  [{ ctrlKey: true }, 2],
  [{ metaKey: true }, 4],
  [{ shiftKey: true }, 8],
  [{ altKey: true, ctrlKey: true, metaKey: true, shiftKey: true }, 15],
];
for (const [modifiers, expected] of maskCases) {
  assert.equal(modifierMask({ ...NO_MODIFIERS, ...modifiers }), expected, JSON.stringify(modifiers));
}

const virtualKeyCases: Array<[string, number]> = [
  ["Backspace", 8], ["Tab", 9], ["Enter", 13], ["Escape", 27], ["PageUp", 33], ["PageDown", 34],
  ["End", 35], ["Home", 36], ["ArrowLeft", 37], ["ArrowUp", 38], ["ArrowRight", 39], ["ArrowDown", 40], ["Delete", 46],
];
for (const [code, expected] of virtualKeyCases) {
  const input = keyInput(key(code, code));
  assert.ok(input?.kind === "key", `${code} maps to a key event`);
  assert.equal(input.windowsVirtualKeyCode, expected, `${code} virtual key code`);
  assert.equal(input.text, code === "Enter" ? "\r" : undefined, `${code} text`);
}
assert.deepEqual(keyInput(key("NumpadEnter", "Enter")), { kind: "key", key: "Enter", code: "NumpadEnter", windowsVirtualKeyCode: 13, modifiers: 0, text: "\r" });
assert.deepEqual(keyInput(key("ArrowLeft", "ArrowLeft", { shiftKey: true, ctrlKey: true })), {
  kind: "key", key: "ArrowLeft", code: "ArrowLeft", windowsVirtualKeyCode: 37, modifiers: MODIFIER.ctrl | MODIFIER.shift,
});
assert.deepEqual(keyInput(key("KeyA", "a")), { kind: "text", text: "a" });
assert.deepEqual(keyInput(key("KeyQ", "@", { ctrlKey: true, altKey: true })), { kind: "text", text: "@" }, "AltGr types its character");
assert.equal(keyInput(key("KeyC", "c", { ctrlKey: true })), null, "Ctrl shortcuts stay with Domios");
assert.equal(keyInput(key("KeyC", "c", { metaKey: true })), null, "Cmd shortcuts stay with Domios");
assert.equal(keyInput(key("F5", "F5")), null, "unmapped named keys are not forwarded");
assert.equal(keyInput(key("ShiftLeft", "Shift", { shiftKey: true })), null, "bare modifiers are not forwarded");

const interactive: KeyContext = { interactive: true, picking: false, origin: "surface" };
const decisionCases: Array<[string, KeyLike, KeyContext, KeyDecision]> = [
  ["Shift+Esc exits interaction", key("Escape", "Escape", { shiftKey: true }), interactive, { action: "exit-interaction" }],
  ["Shift+Esc exits from the address field", key("Escape", "Escape", { shiftKey: true }), { ...interactive, origin: "editable" }, { action: "exit-interaction" }],
  ["Esc is forwarded to the page", key("Escape", "Escape"), interactive, { action: "forward", input: { kind: "key", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, modifiers: 0 } }],
  ["Esc cancels picking", key("Escape", "Escape"), { ...interactive, picking: true }, { action: "cancel-pick" }],
  ["Esc cancels picking while watching", key("Escape", "Escape"), { interactive: false, picking: true, origin: "surface" }, { action: "cancel-pick" }],
  ["Esc in an editable field keeps picking", key("Escape", "Escape"), { interactive: false, picking: true, origin: "editable" }, { action: "keep" }],
  ["address field typing is not forwarded", key("KeyA", "a"), { ...interactive, origin: "editable" }, { action: "keep" }],
  ["address field Enter is not forwarded", key("Enter", "Enter"), { ...interactive, origin: "editable" }, { action: "keep" }],
  ["watch-only keeps every key", key("KeyA", "a"), { ...interactive, interactive: false }, { action: "keep" }],
  ["Shift+Esc while watching is kept", key("Escape", "Escape", { shiftKey: true }), { ...interactive, interactive: false }, { action: "keep" }],
  ["IME composition is kept", { ...key("KeyA", "a"), isComposing: true }, interactive, { action: "keep" }],
  ["text is forwarded", key("KeyA", "A", { shiftKey: true }), interactive, { action: "forward", input: { kind: "text", text: "A" } }],
  ["unmapped shortcut is kept", key("KeyS", "s", { ctrlKey: true }), interactive, { action: "keep" }],
];
for (const [name, event, context, expected] of decisionCases) {
  assert.deepEqual(decideKey(event, context), expected, name);
}

assert.deepEqual(clickInput({ x: 5, y: 6 }, 0), { kind: "click", x: 5, y: 6, button: "left", clickCount: 1 });
assert.deepEqual(clickInput({ x: 5, y: 6 }, 2), { kind: "click", x: 5, y: 6, button: "left", clickCount: 2 });
assert.deepEqual(clickInput({ x: 5, y: 6 }, 9), { kind: "click", x: 5, y: 6, button: "left", clickCount: 3 });

const wheelCases: Array<[number, number]> = [[0, 3], [1, 48], [2, 2400]];
for (const [deltaMode, expectedY] of wheelCases) {
  assert.deepEqual(wheelInput({ x: 1, y: 2 }, { deltaX: 0, deltaY: 3, deltaMode }, 800), { kind: "wheel", x: 1, y: 2, deltaX: 0, deltaY: expectedY }, `wheel mode ${deltaMode}`);
}

assert.deepEqual([...decodeBase64(btoa("\x00\xffAB"))], [0, 255, 65, 66]);

console.log("✓ streamModel tests green");
