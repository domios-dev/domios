import assert from "node:assert/strict";
import { test } from "node:test";
import { COMPOSER_MAX_HEIGHT, resolveComposerAutosize } from "./composerAutosize";
import { COMPOSER_PANEL_HEIGHT_SHARE, resolveComposerLayout } from "./composerLayoutModel";

const draft = {
  focused: true,
  selectionStart: 100,
  selectionEnd: 100,
  textLength: 100,
  contentPx: 800,
  maxPx: 230,
  priorScrollTop: 120,
};

test("typing at the end reveals the end of a capped draft", () => {
  assert.deepEqual(resolveComposerAutosize(draft), { contentPx: 800, editorPx: 230, scrollTop: 570 });
});

test("background measurement preserves the reading position", () => {
  assert.equal(resolveComposerAutosize({ ...draft, focused: false }).scrollTop, 120);
});

test("editing in the middle preserves the caret's existing viewport", () => {
  assert.equal(resolveComposerAutosize({ ...draft, selectionStart: 40, selectionEnd: 40 }).scrollTop, 120);
});

test("a selection ending at the end does not jump to the bottom", () => {
  assert.equal(resolveComposerAutosize({ ...draft, selectionStart: 40 }).scrollTop, 120);
});

test("growing text below the cap needs no internal scroll", () => {
  assert.deepEqual(resolveComposerAutosize({ ...draft, contentPx: 80 }), { contentPx: 80, editorPx: 80, scrollTop: 0 });
});

test("shrinking content clamps an old reading position to the remaining extent", () => {
  assert.deepEqual(resolveComposerAutosize({ ...draft, focused: false, contentPx: 300, priorScrollTop: 570 }),
    { contentPx: 300, editorPx: 230, scrollTop: 70 });
});

test("clearing the draft restores the minimum editor and scroll origin", () => {
  assert.deepEqual(resolveComposerAutosize({ ...draft, contentPx: 0, textLength: 0, selectionStart: 0, selectionEnd: 0 }),
    { contentPx: 0, editorPx: 20, scrollTop: 0 });
});

test("a panel cap retains raw content extent while limiting editor growth", () => {
  assert.deepEqual(resolveComposerAutosize({ ...draft, maxPx: 120 }), { contentPx: 800, editorPx: 120, scrollTop: 680 });
});

test("a cap below the minimum still provides a usable single line", () => {
  assert.deepEqual(resolveComposerAutosize({ ...draft, contentPx: 20, maxPx: 10 }),
    { contentPx: 20, editorPx: 20, scrollTop: 0 });
});

test("scroll activation agrees with autosize at the shared editor cap", () => {
  for (const contentPx of [COMPOSER_MAX_HEIGHT, COMPOSER_MAX_HEIGHT + 1, 800]) {
    const measured = resolveComposerAutosize({ ...draft, contentPx, maxPx: COMPOSER_MAX_HEIGHT });
    // A panel tall enough that the shared editor cap, not the panel share, is the binding limit.
    const layout = resolveComposerLayout({ contentPx, minPx: 20, editorMaxPx: COMPOSER_MAX_HEIGHT,
      panelWidthPx: 1000, panelHeightPx: Math.ceil(COMPOSER_MAX_HEIGHT / COMPOSER_PANEL_HEIGHT_SHARE), keyboardOpen: false });
    assert.equal(layout.editorPx, measured.editorPx);
    assert.equal(layout.scrolls, measured.scrollTop > 0);
  }
});
