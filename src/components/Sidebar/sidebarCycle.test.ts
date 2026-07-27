// Run: npx tsx src/components/Sidebar/sidebarCycle.test.ts

import assert from "node:assert/strict";
import type { Tab, Tab } from "../../lib/types";
import { buildSidebarCycleItems, cycleStep } from "./sidebarCycle";

function makeTab(id: string, tabId: string, order: number): Tab {
  return { id, tab_id: tabId, sort_order: order } as unknown as Tab;
}

function makeTab(id: string): Tab {
  return { id } as unknown as Tab;
}

const tabs: Record<string, Tab> = {
  p1: makeTab("p1"),
  p2: makeTab("p2"),
  p3: makeTab("p3"),
};

const tabs: Tab[] = [
  makeTab("t1", "p1", 0),
  makeTab("t2", "p2", 1),
  makeTab("t3", "p3", 2),
];

// buildSidebarCycleItems

assert.deepEqual(
  buildSidebarCycleItems(tabs, tabs, false),
  [
    { kind: "tab", tabId: "t1" },
    { kind: "tab", tabId: "t2" },
    { kind: "tab", tabId: "t3" },
  ],
  "no plan: returns tabs only",
);

assert.deepEqual(
  buildSidebarCycleItems(tabs, tabs, true)[0],
  { kind: "plan" },
  "with plan: plan entry is first",
);

assert.equal(
  buildSidebarCycleItems(tabs, tabs, true).length,
  4,
  "with plan: total items = plan + tabs",
);

assert.deepEqual(
  buildSidebarCycleItems([], {}, false),
  [],
  "empty tabs + no plan = empty list",
);

assert.deepEqual(
  buildSidebarCycleItems([], {}, true),
  [{ kind: "plan" }],
  "no tabs + plan = plan only",
);

// cycleStep

const items = [
  { kind: "plan" as const },
  { kind: "tab" as const, tabId: "t1" },
  { kind: "tab" as const, tabId: "t2" },
];

assert.equal(
  cycleStep([], "t1", false, 1),
  null,
  "empty items returns null",
);

assert.deepEqual(
  cycleStep(items, undefined, true, 1),
  { kind: "tab", tabId: "t1" },
  "forward from plan → first tab",
);

assert.deepEqual(
  cycleStep(items, "t1", false, 1),
  { kind: "tab", tabId: "t2" },
  "forward from t1 → t2",
);

assert.deepEqual(
  cycleStep(items, "t2", false, 1),
  { kind: "plan" },
  "forward from last tab wraps to plan",
);

assert.deepEqual(
  cycleStep(items, undefined, true, -1),
  { kind: "tab", tabId: "t2" },
  "backward from plan wraps to last tab",
);

assert.deepEqual(
  cycleStep(items, "t1", false, -1),
  { kind: "plan" },
  "backward from t1 → plan",
);

assert.deepEqual(
  cycleStep(items, "t2", false, -1),
  { kind: "tab", tabId: "t1" },
  "backward from t2 → t1",
);

assert.deepEqual(
  cycleStep(items, "unknown", false, 1),
  { kind: "plan" },
  "unknown current (forward) → start from beginning",
);

assert.deepEqual(
  cycleStep(items, "unknown", false, -1),
  { kind: "tab", tabId: "t2" },
  "unknown current (backward) → start from end",
);

const tabOnly = [
  { kind: "tab" as const, tabId: "t1" },
  { kind: "tab" as const, tabId: "t2" },
];

assert.deepEqual(
  cycleStep(tabOnly, "t2", false, 1),
  { kind: "tab", tabId: "t1" },
  "tab-only: wraps forward from last to first",
);

assert.deepEqual(
  cycleStep(tabOnly, "t1", false, -1),
  { kind: "tab", tabId: "t2" },
  "tab-only: wraps backward from first to last",
);

console.log("sidebarCycle.test.ts: ok");
