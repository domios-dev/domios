// WHAT: Enter, Shift+Enter, and IME composition resolve to distinct composer intents.
// WHY: an IME confirmation Enter must never become a provider submit.
// ORACLE: the key-intent function returns submit, newline, or none for the three events.
import assert from "node:assert/strict";
import test from "node:test";

import { composerEnterIntent } from "./ChatComposer";

test("Enter submits", () => {
  assert.equal(composerEnterIntent("Enter", false, false), "submit");
});

test("Shift+Enter inserts a newline without submitting", () => {
  assert.equal(composerEnterIntent("Enter", true, false), "newline");
});

test("IME composition never submits mid-composition", () => {
  assert.equal(composerEnterIntent("Enter", false, true), "none");
});
