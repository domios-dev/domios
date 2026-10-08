import assert from "node:assert/strict";
import { changePreviewKind, changePreviewFailure } from "./changePreviewModel";
import { composerInsertionStore, contextFileName } from "../composerInsertion";
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
assert.deepEqual(scoped.changes, { detail: null, scope: { turnId: "t2" }, view: "diff" });
assert.equal(scoped.documents.detail, "/a.md");
assert.equal(resolveDetail("/a.md", ["/a.md"]), "/a.md");
assert.equal(resolveDetail("/gone.md", ["/a.md"]), null);
assert.equal(resolveDetail(null, ["/a.md"]), null);

tabPanelNavigationStore.dispatch("one", { type: "open", view: "documents", id: "/x.md" });
assert.equal(tabPanelNavigationStore.get("one").documents.detail, "/x.md");
assert.equal(tabPanelNavigationStore.get("two").documents.detail, null);
tabPanelNavigationStore.clear();
assert.equal(tabPanelNavigationStore.get("one"), initialTabPanelState);

composerInsertionStore.insert("tab", [{ kind: "quote", content: "x" }]);
composerInsertionStore.insert("other", [{ kind: "document", path: "/y.md" }]);
composerInsertionStore.insert("tab", [{ kind: "diff", content: "z" }]);
assert.deepEqual(composerInsertionStore.take("tab"), [{ kind: "quote", content: "x" }, { kind: "diff", content: "z" }]);
assert.deepEqual(composerInsertionStore.take("tab"), []);
assert.deepEqual(composerInsertionStore.take("other"), [{ kind: "document", path: "/y.md" }]);
assert.equal(contextFileName({ kind: "quote", detail: "Domios After.md:42" }), "quote-domios-after.md-42.md");
assert.equal(contextFileName({ kind: "diff", label: "diff · A.tsx +2 −1" }), "diff-diff-a.tsx-2-1.diff");
assert.equal(contextFileName({ kind: "message", fileName: "given.md" }), "given.md");

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

const fileView = tabPanelReducer(changesOpen, { type: "change-view", view: "file" });
assert.equal(tabPanelReducer(fileView, { type: "scope", scope: "all" }).changes.view, "file");
assert.equal(tabPanelReducer(fileView, { type: "back", view: "changes" }).changes.view, "file");
assert.equal(tabPanelReducer(fileView, { type: "change-view", view: "file" }), fileView);
for (const path of ["a.ts", "a.rs", "Makefile", "a.md"]) assert.equal(changePreviewKind(path), "document");
for (const path of ["a.HTML", "a.htm"]) assert.equal(changePreviewKind(path), "page");
for (const path of ["a.png", "a.SVG", "a.jpeg"]) assert.equal(changePreviewKind(path), "image");
assert.notEqual(changePreviewFailure("Binary file"), changePreviewFailure("Permission denied"));
assert.notEqual(changePreviewFailure("source exceeds 1000000 bytes"), changePreviewFailure("Permission denied"));
assert.notEqual(changePreviewFailure("Unable to resolve preview file: os error 2"), changePreviewFailure("Permission denied"));
