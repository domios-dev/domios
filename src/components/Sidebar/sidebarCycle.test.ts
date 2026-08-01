// Run: npx tsx src/components/Sidebar/sidebarCycle.test.ts

import assert from "node:assert/strict";
import type { Tab } from "../../lib/types";
import { buildSidebarCycleItems, cycleStep } from "./sidebarCycle";

function tab(id: string, order: number): Tab {
  return { id, sort_order: order } as unknown as Tab;
}

const tabs: Record<string, Tab> = {
  t1: tab("t1", 0),
  t2: tab("t2", 1),
  t3: tab("t3", 2),
};
const tabIds = ["t1", "t2", "t3"];

assert.deepEqual(
  buildSidebarCycleItems(tabIds, tabs, false),
  [
    { kind: "tab", tabId: "t1" },
    { kind: "tab", tabId: "t2" },
    { kind: "tab", tabId: "t3" },
  ],
);
assert.deepEqual(buildSidebarCycleItems(tabIds, tabs, true)[0], { kind: "plan" });
assert.equal(buildSidebarCycleItems(tabIds, tabs, true).length, 4);
assert.deepEqual(buildSidebarCycleItems([], {}, false), []);
assert.deepEqual(buildSidebarCycleItems([], {}, true), [{ kind: "plan" }]);

const items = [
  { kind: "plan" as const },
  { kind: "tab" as const, tabId: "t1" },
  { kind: "tab" as const, tabId: "t2" },
];
assert.equal(cycleStep([], "t1", false, 1), null);
assert.deepEqual(cycleStep(items, undefined, true, 1), { kind: "tab", tabId: "t1" });
assert.deepEqual(cycleStep(items, "t1", false, 1), { kind: "tab", tabId: "t2" });
assert.deepEqual(cycleStep(items, "t2", false, 1), { kind: "plan" });
assert.deepEqual(cycleStep(items, undefined, true, -1), { kind: "tab", tabId: "t2" });
assert.deepEqual(cycleStep(items, "t1", false, -1), { kind: "plan" });
assert.deepEqual(cycleStep(items, "t2", false, -1), { kind: "tab", tabId: "t1" });
assert.deepEqual(cycleStep(items, "unknown", false, 1), { kind: "plan" });
assert.deepEqual(cycleStep(items, "unknown", false, -1), { kind: "tab", tabId: "t2" });

const tabOnly = items.slice(1);
assert.deepEqual(cycleStep(tabOnly, "t2", false, 1), { kind: "tab", tabId: "t1" });
assert.deepEqual(cycleStep(tabOnly, "t1", false, -1), { kind: "tab", tabId: "t2" });

console.log("sidebarCycle.test.ts: ok");
