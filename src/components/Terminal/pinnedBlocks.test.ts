// Run: npx tsx src/components/Terminal/pinnedBlocks.test.ts

import assert from "node:assert/strict";
import { Window } from "happy-dom";

const testWindow = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: testWindow, configurable: true });

void (async () => {
const { pinnedBlocksStore, togglePinned } = await import("./pinnedBlocks");

assert.deepEqual(togglePinned([], "@ds-plugin a"), ["@ds-plugin a"]);
assert.deepEqual(togglePinned(["@ds-plugin a", "@ds-plugin b"], "@ds-plugin a"), ["@ds-plugin b"]);

pinnedBlocksStore.resetForTest();
let notified = 0;
const unsubscribe = pinnedBlocksStore.subscribe(() => notified++);
assert.equal(pinnedBlocksStore.toggle("tab-1", "@ds-plugin telegram-client"), true);
assert.deepEqual(pinnedBlocksStore.list("tab-1"), ["@ds-plugin telegram-client"]);
assert.deepEqual(pinnedBlocksStore.list("tab-2"), [], "pins are per tab");
assert.equal(pinnedBlocksStore.list("tab-1"), pinnedBlocksStore.list("tab-1"), "list must be stable for useSyncExternalStore");
assert.equal(pinnedBlocksStore.toggle("tab-1", "@ds-plugin telegram-client"), false);
assert.deepEqual(pinnedBlocksStore.list("tab-1"), []);
assert.equal(notified, 2);
unsubscribe();

console.log("pinnedBlocks: passed");
})();
