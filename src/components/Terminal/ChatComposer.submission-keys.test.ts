// WHAT: Enter, Shift+Enter, and IME composition resolve to distinct composer intents,
// and the composition gate classifies engine-ordered key events.
// WHY: an IME confirmation Enter must never become a provider submit — WebKit
// delivers it AFTER compositionend with isComposing=false but keyCode 229, and
// mid-composition layout writes disturb the WebKitGTK preedit.
// ORACLE: the pure intent/gate functions' return values for each event shape.
import assert from "node:assert/strict";
import test from "node:test";

import { composerEnterIntent } from "./ChatComposer";
import {
  beginComposition,
  compositionDefersAutosize,
  createCompositionGate,
  endComposition,
  isImeKeyEvent,
} from "./composerComposition";

test("Enter submits", () => {
  assert.equal(composerEnterIntent("Enter", false, false), "submit");
});

test("Shift+Enter inserts a newline without submitting", () => {
  assert.equal(composerEnterIntent("Enter", true, false), "newline");
});

test("IME composition never submits mid-composition", () => {
  assert.equal(composerEnterIntent("Enter", false, true), "none");
});

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
