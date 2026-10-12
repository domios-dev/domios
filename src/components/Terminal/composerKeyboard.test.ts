import assert from "node:assert/strict";
import { test } from "node:test";
import { classifyComposerDevice, composerEnterIntent, type ComposerDevice, type ComposerKey } from "./composerKeyboard";

const enterShapes = [
  { code: "Enter" },
  { code: "NumpadEnter" },
  { code: "Enter", key: "Enter", keyCode: 13 },
  { code: "NumpadEnter", key: "Enter" },
  { code: "", key: "Enter", keyCode: 13 },
  { code: "Unidentified", key: "Enter", keyCode: 13 },
];
const otherShapes = [
  { code: "KeyA", key: "Enter" },
  { code: "", key: "a", keyCode: 13 },
  { code: "Unidentified", key: "Unidentified", keyCode: 13 },
  { code: "" },
];
const unmodified = { shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, isComposing: false };

for (const device of ["ipad", "phone", "desktop"] satisfies ComposerDevice[]) {
  for (const shape of enterShapes) {
    test(`${device}: ${JSON.stringify(shape)} respects modifiers and IME`, () => {
      const key: ComposerKey = { ...unmodified, ...shape };
      assert.equal(composerEnterIntent(device, key), device === "phone" ? "newline" : "submit");
      assert.equal(composerEnterIntent(device, { ...key, shiftKey: true }), "newline");
      assert.equal(composerEnterIntent(device, { ...key, isComposing: true }), "none");
      assert.equal(composerEnterIntent(device, { ...key, shiftKey: true, isComposing: true }), "none");
      assert.equal(composerEnterIntent(device, { ...key, keyCode: 229 }), "none");
      for (const modifier of ["ctrlKey", "metaKey", "altKey"] as const) {
        assert.equal(composerEnterIntent(device, { ...key, [modifier]: true }), "none");
      }
    });
  }
  for (const shape of otherShapes) {
    test(`${device}: ${JSON.stringify(shape)} is not Enter`, () => {
      assert.equal(composerEnterIntent(device, { ...unmodified, ...shape }), "none");
    });
  }
}

const devices = [
  { name: "iPad Safari", userAgent: "Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X)", platform: "iPad", maxTouchPoints: 5, touchOptimizedUi: true, expected: "ipad" },
  { name: "iPad installed PWA", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15) AppleWebKit/605.1.15 Mobile/15E148", platform: "MacIntel", maxTouchPoints: 5, touchOptimizedUi: true, expected: "ipad" },
  { name: "iPad desktop Safari", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15) Version/18.0 Safari/605.1.15", platform: "MacIntel", maxTouchPoints: 5, touchOptimizedUi: true, expected: "ipad" },
  { name: "iPad hardware keyboard", userAgent: "Mozilla/5.0 (Macintosh)", platform: "MacIntel", maxTouchPoints: 5, touchOptimizedUi: false, expected: "ipad" },
  { name: "iPad Macintosh UA without platform", userAgent: "Mozilla/5.0 (Macintosh)", platform: "", maxTouchPoints: 5, touchOptimizedUi: false, expected: "ipad" },
  { name: "iPhone hardware keyboard", userAgent: "Mozilla/5.0 (iPhone)", platform: "iPhone", maxTouchPoints: 5, touchOptimizedUi: false, expected: "phone" },
  { name: "Android phone", userAgent: "Mozilla/5.0 (Linux; Android 15)", platform: "Linux armv8l", maxTouchPoints: 5, touchOptimizedUi: true, expected: "phone" },
  { name: "Mac desktop", userAgent: "Mozilla/5.0 (Macintosh)", platform: "MacIntel", maxTouchPoints: 0, touchOptimizedUi: false, expected: "desktop" },
  { name: "Mac one touch point", userAgent: "Mozilla/5.0 (Macintosh)", platform: "MacIntel", maxTouchPoints: 1, touchOptimizedUi: false, expected: "desktop" },
  { name: "Windows desktop", userAgent: "Mozilla/5.0 (Windows NT 10.0)", platform: "Win32", maxTouchPoints: 0, touchOptimizedUi: false, expected: "desktop" },
  { name: "Linux desktop", userAgent: "Mozilla/5.0 (X11; Linux x86_64)", platform: "Linux x86_64", maxTouchPoints: 0, touchOptimizedUi: false, expected: "desktop" },
] satisfies Array<{ name: string; userAgent: string; platform: string; maxTouchPoints: number; touchOptimizedUi: boolean; expected: ComposerDevice }>;

for (const device of devices) {
  test(`${device.name} classifies as ${device.expected}`, () => {
    assert.equal(classifyComposerDevice(device), device.expected);
  });
}
