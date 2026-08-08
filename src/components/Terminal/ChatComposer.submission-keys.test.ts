// WHAT: Enter, Shift+Enter, and IME composition resolve to distinct composer intents,
// and the composition gate classifies engine-ordered key/change events.
// WHY: an IME confirmation Enter must never become a provider submit — WebKit
// delivers it AFTER compositionend with isComposing=false but keyCode 229, and
// committing intermediate IME text to React state re-inserts prefixes on WebKitGTK.
// ORACLE: the pure intent/gate functions' return values for each event shape.
import assert from "node:assert/strict";
import test from "node:test";

import { composerEnterIntent } from "./ChatComposer";
import {
  beginComposition,
  compositionRenderValue,
  createCompositionGate,
  deferCompositionChange,
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
  beginComposition(gate, "");
  assert.equal(isImeKeyEvent(gate, true, 229), true);
});

test("WebKit commit-Enter after compositionend (isComposing=false, keyCode 229) is an IME key", () => {
  const gate = createCompositionGate();
  beginComposition(gate, "");
  endComposition(gate, "한");
  assert.equal(isImeKeyEvent(gate, false, 229), true);
});

test("the open gate alone marks a key as IME even without engine flags", () => {
  const gate = createCompositionGate();
  beginComposition(gate, "");
  assert.equal(isImeKeyEvent(gate, false, 13), true);
});

test("plain Enter (keyCode 13) with the gate closed is not an IME key", () => {
  const gate = createCompositionGate();
  assert.equal(isImeKeyEvent(gate, false, 13), false);
  assert.equal(isImeKeyEvent(gate, false, undefined), false);
});

test("intermediate composition changes are deferred; plain changes are not", () => {
  const gate = createCompositionGate();
  assert.equal(deferCompositionChange(gate, false, "a"), false);
  beginComposition(gate, "");
  assert.equal(deferCompositionChange(gate, true, "р"), true);
  assert.equal(deferCompositionChange(gate, false, "ра"), true);
  assert.equal(gate.domValue, "ра");
});

test("a composition commits its final value exactly once at compositionend", () => {
  const gate = createCompositionGate();
  beginComposition(gate, "");
  deferCompositionChange(gate, true, "р");
  deferCompositionChange(gate, true, "ра");
  assert.equal(endComposition(gate, "рас"), "рас");
  assert.equal(gate.active, false);
  assert.equal(deferCompositionChange(gate, false, "рас"), false);
});

test("while composing the rendered value mirrors the DOM, never the stale draft", () => {
  const gate = createCompositionGate();
  beginComposition(gate, "");
  deferCompositionChange(gate, true, "р");
  assert.equal(compositionRenderValue(gate, ""), "р");
  endComposition(gate, "р");
  assert.equal(compositionRenderValue(gate, "р"), "р");
});
