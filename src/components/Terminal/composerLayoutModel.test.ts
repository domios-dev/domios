import assert from "node:assert/strict";
import { test } from "node:test";
import { resolveComposerLayout } from "./composerLayoutModel";

const base = { contentPx: 20, minPx: 20, editorMaxPx: 230, panelWidthPx: 900, panelHeightPx: 800, keyboardOpen: false };
test("stays a floating capsule through any multiline growth", () => {
  assert.deepEqual(resolveComposerLayout({ ...base, contentPx: 229 }), { mode: "floating", editorPx: 229, scrolls: false });
  assert.deepEqual(resolveComposerLayout({ ...base, contentPx: 230 }), { mode: "floating", editorPx: 230, scrolls: false });
  assert.deepEqual(resolveComposerLayout({ ...base, contentPx: 400 }), { mode: "floating", editorPx: 230, scrolls: true });
});
test("narrow and keyboard-open panels stay docked even when clear", () => {
  assert.equal(resolveComposerLayout({ ...base, panelWidthPx: 360 }).mode, "docked");
  assert.equal(resolveComposerLayout({ ...base, keyboardOpen: true }).mode, "docked");
  assert.equal(resolveComposerLayout({ ...base, panelWidthPx: 480 }).mode, "floating");
});
test("editor max respects 40 percent of panel height and never falls below minimum", () => {
  assert.deepEqual(resolveComposerLayout({ ...base, panelHeightPx: 300, contentPx: 180 }), { mode: "floating", editorPx: 120, scrolls: true });
  assert.equal(resolveComposerLayout({ ...base, panelHeightPx: 10 }).editorPx, 20);
});
test("chrome and attachment preview heights cannot affect the editor model", () => {
  const withAttachments = { ...base, attachmentPreviewPx: 500 };
  assert.deepEqual(resolveComposerLayout(withAttachments), resolveComposerLayout(base));
});
