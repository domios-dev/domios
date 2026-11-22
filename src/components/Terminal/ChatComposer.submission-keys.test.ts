// The composition gate must distinguish IME confirmation from provider submit because WebKit reports Enter after compositionend.
import assert from "node:assert/strict";
import { test, vi } from "vitest";

import { classifyComposerDevice, composerEnterIntent, type ComposerDevice, type ComposerKey } from "./composerKeyboard";
import {
  beginComposition,
  compositionDefersAutosize,
  createCompositionGate,
  endComposition,
  isImeKeyEvent,
} from "./composerComposition";
import { createSpaceDictationHold, SPACE_DICTATION_HOLD_MS, type SpaceDictationPhase } from "./voice/spaceDictationHold";

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

const plainSpace = (repeat = false) => ({
  code: "Space", repeat, shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, isComposing: false,
});
const emptyCtx = { empty: true, dictating: false, pickerOpen: false, composing: false };

test("hold-Space push-to-talk: quick tap <300ms inserts a space without starting dictation", () => {
  vi.useFakeTimers();
  try {
    const phases: SpaceDictationPhase[] = [];
    let starts = 0, stops = 0, spaces = 0, pops = 0;
    const hold = createSpaceDictationHold({
      onPhaseChange: (phase) => phases.push(phase),
      onStartDictation: () => { starts++; },
      onStopDictation: () => { stops++; },
      onInsertSpace: () => { spaces++; },
      onPop: () => { pops++; },
    });

    assert.equal(hold.onKeyDown(plainSpace(false), emptyCtx), true);
    assert.equal(hold.phase(), "arming");
    vi.advanceTimersByTime(SPACE_DICTATION_HOLD_MS - 50);
    hold.onKeyUp("Space", true);
    vi.advanceTimersByTime(SPACE_DICTATION_HOLD_MS);

    assert.equal(starts, 0);
    assert.equal(stops, 0);
    assert.equal(pops, 0);
    assert.equal(spaces, 1);
    assert.deepEqual(phases, ["arming", "idle"]);
  } finally {
    vi.useRealTimers();
  }
});

test("hold-Space push-to-talk: >=300ms ignores repeats, pops and starts dictation, and keyup stops it", () => {
  vi.useFakeTimers();
  try {
    const phases: SpaceDictationPhase[] = [];
    let starts = 0, stops = 0, spaces = 0, pops = 0;
    const hold = createSpaceDictationHold({
      onPhaseChange: (phase) => phases.push(phase),
      onStartDictation: () => { starts++; },
      onStopDictation: () => { stops++; },
      onInsertSpace: () => { spaces++; },
      onPop: () => { pops++; },
    });

    assert.equal(hold.onKeyDown(plainSpace(false), emptyCtx), true);
    assert.equal(hold.onKeyDown(plainSpace(true), emptyCtx), true, "OS repeats must be swallowed while arming");
    vi.advanceTimersByTime(SPACE_DICTATION_HOLD_MS);
    assert.equal(hold.phase(), "recording");
    assert.equal(starts, 1);
    assert.equal(pops, 1);
    assert.equal(hold.onKeyDown(plainSpace(true), { ...emptyCtx, dictating: true }), true, "OS repeats must be swallowed while recording");

    hold.onKeyUp("Space", true);
    assert.equal(hold.phase(), "idle");
    assert.equal(stops, 1);
    assert.equal(spaces, 0);
    assert.deepEqual(phases, ["arming", "recording", "idle"]);
  } finally {
    vi.useRealTimers();
  }
});

test("hold-Space push-to-talk: non-empty input and blur behave cleanly", () => {
  vi.useFakeTimers();
  try {
    let starts = 0, stops = 0, spaces = 0;
    const hold = createSpaceDictationHold({
      onPhaseChange: () => {},
      onStartDictation: () => { starts++; },
      onStopDictation: () => { stops++; },
      onInsertSpace: () => { spaces++; },
      onPop: () => {},
    });

    assert.equal(hold.onKeyDown(plainSpace(false), { ...emptyCtx, empty: false }), false);
    assert.equal(hold.phase(), "idle");
    vi.advanceTimersByTime(SPACE_DICTATION_HOLD_MS * 2);
    assert.equal(starts, 0);

    // Blur while arming cancels without opening mic or inserting space.
    assert.equal(hold.onKeyDown(plainSpace(false), emptyCtx), true);
    vi.advanceTimersByTime(100);
    hold.onBlur();
    vi.advanceTimersByTime(SPACE_DICTATION_HOLD_MS);
    assert.equal(hold.phase(), "idle");
    assert.equal(starts, 0);
    assert.equal(spaces, 0);

    // Blur while recording stops cleanly so spoken words commit.
    assert.equal(hold.onKeyDown(plainSpace(false), emptyCtx), true);
    vi.advanceTimersByTime(SPACE_DICTATION_HOLD_MS);
    assert.equal(starts, 1);
    hold.onBlur();
    assert.equal(stops, 1);
    assert.equal(hold.phase(), "idle");
  } finally {
    vi.useRealTimers();
  }
});
