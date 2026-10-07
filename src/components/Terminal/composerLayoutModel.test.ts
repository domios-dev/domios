import assert from "node:assert/strict";
import { test } from "node:test";
import { resolveComposerLayout } from "./composerLayoutModel";

const base = { contentPx: 20, minPx: 20, editorMaxPx: 230, panelWidthPx: 900, panelHeightPx: 800, keyboardOpen: false };
test("floats through multiline growth and docks exactly at editor max", () => {
  assert.deepEqual(resolveComposerLayout({ ...base, contentPx: 229 }), { mode: "floating", editorPx: 229, scrolls: false });
  assert.deepEqual(resolveComposerLayout({ ...base, contentPx: 230 }), { mode: "docked", editorPx: 230, scrolls: false });
  assert.deepEqual(resolveComposerLayout({ ...base, contentPx: 400 }), { mode: "docked", editorPx: 230, scrolls: true });
});
test("holds the dock through rewrap with 40px hysteresis and releases below it", () => {
  assert.equal(resolveComposerLayout({ ...base, contentPx: 190, prev: "docked" }).mode, "docked");
  assert.equal(resolveComposerLayout({ ...base, contentPx: 189, prev: "docked" }).mode, "floating");
  assert.equal(resolveComposerLayout({ ...base, contentPx: 20, prev: "docked" }).mode, "floating");
});
test("narrow and keyboard-open panels stay docked even when clear", () => {
  assert.equal(resolveComposerLayout({ ...base, panelWidthPx: 360 }).mode, "docked");
  assert.equal(resolveComposerLayout({ ...base, keyboardOpen: true }).mode, "docked");
  assert.equal(resolveComposerLayout({ ...base, panelWidthPx: 480 }).mode, "floating");
});
test("editor max respects 40 percent of panel height and never falls below minimum", () => {
  assert.deepEqual(resolveComposerLayout({ ...base, panelHeightPx: 300, contentPx: 180 }), { mode: "docked", editorPx: 120, scrolls: true });
  assert.equal(resolveComposerLayout({ ...base, panelHeightPx: 10 }).editorPx, 20);
});
test("chrome and attachment preview heights cannot affect the editor model", () => {
  const withAttachments = { ...base, attachmentPreviewPx: 500 };
  assert.deepEqual(resolveComposerLayout(withAttachments), resolveComposerLayout(base));
});
