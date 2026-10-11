// The composition gate must distinguish IME confirmation from provider submit because WebKit reports Enter after compositionend.
import assert from "node:assert/strict";
import { test } from "vitest";

import { classifyComposerDevice, composerEnterIntent, type ComposerDevice, type ComposerKey } from "./composerKeyboard";
import {
  beginComposition,
  compositionDefersAutosize,
  createCompositionGate,
  endComposition,
  isImeKeyEvent,
} from "./composerComposition";

test("Chromium commit-Enter (isComposing=true, before compositionend) is an IME key", () => {
  const gate = createCompositionGate();
  beginComposition(gate);
  assert.equal(isImeKeyEvent(gate, true, 229), true);
});

test("WebKit commit-Enter after compositionend (isComposing=false, keyCode 229) is an IME key", () => {
  const gate = createCompositionGate();
  beginComposition(gate);
  endComposition(gate);
  assert.equal(isImeKeyEvent(gate, false, 229), true);
});

test("the open gate alone marks a key as IME even without engine flags", () => {
  const gate = createCompositionGate();
  beginComposition(gate);
  assert.equal(isImeKeyEvent(gate, false, 13), true);
});

test("plain Enter (keyCode 13) with the gate closed is not an IME key", () => {
  const gate = createCompositionGate();
  assert.equal(isImeKeyEvent(gate, false, 13), false);
  assert.equal(isImeKeyEvent(gate, false, undefined), false);
});

test("autosize is deferred only while a composition is active", () => {
  const gate = createCompositionGate();
  assert.equal(compositionDefersAutosize(gate), false);
  beginComposition(gate);
  assert.equal(compositionDefersAutosize(gate), true);
  endComposition(gate);
  assert.equal(compositionDefersAutosize(gate), false);
});

const devices = [
  { name: "desktop macOS", platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 0, touchOptimizedUi: false, expected: "desktop" },
  { name: "Windows desktop", platform: "Win32", userAgent: "Mozilla/5.0 (Windows NT 10.0)", maxTouchPoints: 0, touchOptimizedUi: false, expected: "desktop" },
  { name: "Linux desktop", platform: "Linux x86_64", userAgent: "Mozilla/5.0 (X11; Linux x86_64)", maxTouchPoints: 0, touchOptimizedUi: false, expected: "desktop" },
  { name: "iPad desktop Safari", platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 5, touchOptimizedUi: true, expected: "ipad" },
  { name: "iPad with external keyboard and trackpad", platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", maxTouchPoints: 5, touchOptimizedUi: false, expected: "ipad" },
  { name: "iPad mobile Safari", platform: "iPad", userAgent: "Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X)", maxTouchPoints: 5, touchOptimizedUi: true, expected: "ipad" },
  { name: "iPhone", platform: "iPhone", userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)", maxTouchPoints: 5, touchOptimizedUi: true, expected: "phone" },
  { name: "iPhone external keyboard", platform: "iPhone", userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)", maxTouchPoints: 5, touchOptimizedUi: false, expected: "phone" },
  { name: "Android touch device", platform: "Linux armv8l", userAgent: "Mozilla/5.0 (Linux; Android 15)", maxTouchPoints: 5, touchOptimizedUi: true, expected: "phone" },
  { name: "Mac with one touch point", platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh)", maxTouchPoints: 1, touchOptimizedUi: false, expected: "desktop" },
] satisfies Array<{ name: string; platform: string; userAgent: string; maxTouchPoints: number; touchOptimizedUi: boolean; expected: ComposerDevice }>;

for (const device of devices) {
  test(`${device.name}: device, modifiers, Enter codes and IME matrix`, () => {
    const classified = classifyComposerDevice(device);
    assert.equal(classified, device.expected);
    for (const code of ["Enter", "NumpadEnter", "KeyA"]) {
      for (let modifiers = 0; modifiers < 16; modifiers += 1) {
        for (const isComposing of [false, true]) {
          const key: ComposerKey = {
            code, shiftKey: Boolean(modifiers & 1), ctrlKey: Boolean(modifiers & 2),
            metaKey: Boolean(modifiers & 4), altKey: Boolean(modifiers & 8), isComposing,
          };
          const expected = isComposing || code === "KeyA" || (modifiers & 14) !== 0
            ? "none" : key.shiftKey || device.expected === "phone" ? "newline" : "submit";
          assert.equal(composerEnterIntent(classified, key), expected, JSON.stringify({ device: device.name, key }));
        }
      }
    }
  });
}
