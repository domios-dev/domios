// Run: npx tsx src/components/Terminal/pinnedBlocks.test.ts

import assert from "node:assert/strict";
import { Window } from "happy-dom";

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: testWindow, configurable: true });

void (async () => {
const { pinnedBlocksStore, togglePinned, openRecordedFileChange } = await import("./pinnedBlocks");
const { chatChangesStore } = await import("./chatChangesStore");
const { tabPanelNavigationStore } = await import("./tabPanel/tabPanelNavigation");
const { planSidePanelStore } = await import("../Canvas/planSidePanelStore");

assert.deepEqual(togglePinned([], "<ds-plugin id='a' />"), ["<ds-plugin id='a' />"]);
assert.deepEqual(togglePinned(["<ds-plugin id='a' />", "<ds-plugin id='b' />"], "<ds-plugin id='a' />"), ["<ds-plugin id='b' />"]);

pinnedBlocksStore.resetForTest();
let notified = 0;
const unsubscribe = pinnedBlocksStore.subscribe(() => notified++);
assert.equal(pinnedBlocksStore.toggle("tab-1", "<ds-plugin id='telegram-client' />"), true);
assert.deepEqual(pinnedBlocksStore.list("tab-1"), ["<ds-plugin id='telegram-client' />"]);
assert.deepEqual(pinnedBlocksStore.list("tab-2"), [], "pins are per tab");
assert.equal(pinnedBlocksStore.list("tab-1"), pinnedBlocksStore.list("tab-1"), "list must be stable for useSyncExternalStore");
assert.equal(pinnedBlocksStore.toggle("tab-1", "<ds-plugin id='telegram-client' />"), false);
assert.deepEqual(pinnedBlocksStore.list("tab-1"), []);
assert.equal(notified, 2);
unsubscribe();

chatChangesStore.set("change-owner", [{
  id: "edit", type: "tool_call", content: "", toolStatus: "completed", toolResult: "done",
  fileDiffs: [{ path: "C:/work/old.ts", movePath: "C:/work/new.ts", additions: 1, deletions: 1,
    hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-old", "+new"] }] }],
}]);
openRecordedFileChange("other-tab", "C:/work/new.ts", "file");
assert.equal(planSidePanelStore.get(), null, "another tab cannot open an unrecorded file");
openRecordedFileChange("change-owner", "C:\\work\\old.ts", "file");
assert.equal(planSidePanelStore.get()?.target.documentsTabId, "change-owner");
assert.equal(planSidePanelStore.get()?.target.tabView, "changes");
assert.deepEqual(tabPanelNavigationStore.get("change-owner").changes, { detail: "C:/work/new.ts", scope: "all", view: "file" });
openRecordedFileChange("change-owner", "C:/work/new.ts", "diff");
assert.equal(tabPanelNavigationStore.get("change-owner").changes.view, "diff");
planSidePanelStore.close();
chatChangesStore.clear();
tabPanelNavigationStore.clear();

console.log("pinnedBlocks: passed");
})();
