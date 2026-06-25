// Run: npx tsx src/components/Sidebar/workspaceTabs.test.ts

import assert from "node:assert/strict";
import type { Pane, Tab } from "../../lib/types";
import { childPaneIdsInWorkspace, topLevelTabsInWorkspace } from "./workspaceTabs";

function tab(id: string, paneId: string, sortOrder = 0): Tab {
  return { id, pane_id: paneId, sort_order: sortOrder };
}

function pane(id: string, parentPaneId?: string): Pane {
  return {
    id,
    title: id,
    pane_type: "terminal",
    title_level: "Default",
    parent_pane_id: parentPaneId ?? null,
    depth: parentPaneId ? 1 : 0,
  } as unknown as Pane;
}

const parentTab = tab("tab-parent", "parent-pane", 0);
const childTab = tab("tab-child", "child-pane", 1);
const movedChildTab = tab("tab-moved", "moved-pane", 2);

assert.deepEqual(
  topLevelTabsInWorkspace([parentTab, childTab], {
    "parent-pane": pane("parent-pane"),
    "child-pane": pane("child-pane", "parent-pane"),
  }).map((t) => t.id),
  ["tab-parent"],
  "in-workspace child nests under parent",
);

assert.deepEqual(
  topLevelTabsInWorkspace([movedChildTab], {
    "moved-pane": pane("moved-pane", "parent-pane"),
  }).map((t) => t.id),
  ["tab-moved"],
  "cross-workspace child surfaces at top level after move-tab",
);

assert.equal(
  childPaneIdsInWorkspace([movedChildTab], {
    "moved-pane": pane("moved-pane", "parent-pane"),
  }).size,
  0,
  "cross-workspace child is not treated as nested",
);

console.log("workspaceTabs.test.ts: ok");
