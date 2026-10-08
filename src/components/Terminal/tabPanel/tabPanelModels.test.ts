import assert from "node:assert/strict";
import { appendComposerBlock, applyComposerInsertions, composerInsertionStore } from "../composerInsertion";
import { lineSuffix, locateSelection, quotedSelection } from "./selectionReference";
import { initialTabPanelState, resolveDetail, tabPanelNavigationStore, tabPanelReducer } from "./tabPanelNavigation";

const opened = tabPanelReducer(initialTabPanelState, { type: "open", view: "documents", id: "/a.md" });
assert.equal(opened.documents.detail, "/a.md");
assert.equal(opened.changes.detail, null);
assert.equal(tabPanelReducer(opened, { type: "open", view: "documents", id: "/a.md" }), opened);
assert.equal(tabPanelReducer(opened, { type: "back", view: "documents" }).documents.detail, null);
assert.equal(tabPanelReducer(initialTabPanelState, { type: "back", view: "changes" }), initialTabPanelState);
const changesOpen = tabPanelReducer(opened, { type: "open", view: "changes", id: "src/a.ts" });
const scoped = tabPanelReducer(changesOpen, { type: "scope", scope: { turnId: "t2" } });
assert.deepEqual(scoped.changes, { detail: null, scope: { turnId: "t2" } });
assert.equal(scoped.documents.detail, "/a.md");
assert.equal(resolveDetail("/a.md", ["/a.md"]), "/a.md");
assert.equal(resolveDetail("/gone.md", ["/a.md"]), null);
assert.equal(resolveDetail(null, ["/a.md"]), null);

tabPanelNavigationStore.dispatch("one", { type: "open", view: "documents", id: "/x.md" });
assert.equal(tabPanelNavigationStore.get("one").documents.detail, "/x.md");
assert.equal(tabPanelNavigationStore.get("two").documents.detail, null);
tabPanelNavigationStore.clear();
assert.equal(tabPanelNavigationStore.get("one"), initialTabPanelState);

assert.equal(appendComposerBlock("", "block"), "block\n");
assert.equal(appendComposerBlock("draft  \n", "block\n\n"), "draft\n\nblock\n");
assert.equal(appendComposerBlock("draft", "  "), "draft");
const applied = applyComposerInsertions("hi", [{ attachments: ["/a.md"] }, { text: "one" }, { attachments: ["/a.md", "/b.md"], text: "two" }]);
assert.deepEqual(applied, { draft: "hi\n\none\n\ntwo\n", attachments: ["/a.md", "/b.md"] });
composerInsertionStore.insert("tab", { text: "x" });
composerInsertionStore.insert("other", { text: "y" });
assert.deepEqual(composerInsertionStore.take("tab"), [{ text: "x" }]);
assert.deepEqual(composerInsertionStore.take("tab"), []);
assert.deepEqual(composerInsertionStore.take("other"), [{ text: "y" }]);

const source = "# Title\n\nIntro with **bold** text.\n\n- item one\n- item two\n";
assert.deepEqual(locateSelection(source, "Intro with bold text."), { start: 3, end: 3 });
assert.deepEqual(locateSelection(source, "item one\nitem two"), { start: 5, end: 6 });
assert.deepEqual(locateSelection(source, "Title"), { start: 1, end: 1 });
assert.equal(locateSelection(source, "not present"), null);
assert.equal(locateSelection(source, "  \n "), null);
assert.equal(lineSuffix({ start: 5, end: 6 }), ":5-6");
assert.equal(lineSuffix({ start: 3, end: 3 }), ":3");
assert.equal(lineSuffix(null), "");
assert.equal(quotedSelection("~/a.md", "item one\n\nitem two\n", { start: 5, end: 6 }), "From `~/a.md:5-6`:\n> item one\n>\n> item two");
console.log("Tab panel model checks passed");
