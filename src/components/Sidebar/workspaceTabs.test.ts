// Run: npx tsx src/components/Sidebar/workspaceTabs.test.ts

import assert from "node:assert/strict";
import type { Pane, Tab } from "../../lib/types";
import {
  childPaneIdsInWorkspace,
  childTabsForParent,
  topLevelTabsInWorkspace,
} from "./workspaceTabs";

function tab(id: string, paneId: string, sortOrder = 0): Tab {
  return { id, pane_id: paneId, sort_order: sortOrder };
}

function pane(
  id: string,
  parentPaneId?: string,
  cwd?: string | null,
  hidden?: boolean,
): Pane {
  return {
    id,
    title: id,
    pane_type: "terminal",
    title_level: "Default",
    parent_pane_id: parentPaneId ?? null,
    depth: parentPaneId ? 1 : 0,
    cwd: cwd ?? null,
    hidden: hidden ?? false,
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
  childTabsForParent([parentTab, childTab], {
    "parent-pane": pane("parent-pane", undefined, "/workspace"),
    "child-pane": pane("child-pane", "parent-pane", "/other/project"),
  }, "parent-pane").map((t) => t.id),
  ["tab-child"],
  "child nests under parent when cwd differs from workspace",
);

assert.deepEqual(
  topLevelTabsInWorkspace([movedChildTab], {
    "moved-pane": pane("moved-pane", "parent-pane"),
  }).map((t) => t.id),
  ["tab-moved"],
  "cross-workspace child surfaces at top level after move-pane",
);

assert.equal(
  childPaneIdsInWorkspace([movedChildTab], {
    "moved-pane": pane("moved-pane", "parent-pane"),
  }).size,
  0,
  "cross-workspace child is not treated as nested",
);

const grandparentTab = tab("tab-gp", "gp-pane", 0);
const midTab = tab("tab-mid", "mid-pane", 1);
const leafTab = tab("tab-leaf", "leaf-pane", 2);

assert.deepEqual(
  topLevelTabsInWorkspace([grandparentTab, midTab, leafTab], {
    "gp-pane": pane("gp-pane"),
    "mid-pane": pane("mid-pane", "gp-pane"),
    "leaf-pane": pane("leaf-pane", "mid-pane"),
  }).map((t) => t.id),
  ["tab-gp"],
  "only the hierarchy root is top-level",
);

assert.deepEqual(
  childTabsForParent([grandparentTab, midTab, leafTab], {
    "gp-pane": pane("gp-pane"),
    "mid-pane": pane("mid-pane", "gp-pane"),
    "leaf-pane": pane("leaf-pane", "mid-pane"),
  }, "mid-pane").map((t) => t.id),
  ["tab-leaf"],
  "grandchild renders under its direct parent",
);

const hiddenParentTab = tab("tab-hidden-parent", "hidden-parent", 0);
const visibleChildTab = tab("tab-visible-child", "visible-child", 1);

assert.deepEqual(
  topLevelTabsInWorkspace([hiddenParentTab, visibleChildTab], {
    "hidden-parent": pane("hidden-parent", undefined, null, true),
    "visible-child": pane("visible-child", "hidden-parent"),
  }).map((t) => t.id),
  ["tab-visible-child"],
  "child of hidden parent surfaces at top level",
);

assert.deepEqual(
  topLevelTabsInWorkspace([parentTab, tab("tab-orphan", "missing-pane", 1)], {
    "parent-pane": pane("parent-pane"),
  }).map((t) => t.id),
  ["tab-parent"],
  "tab without a Tab record does not surface at top level",
);

assert.deepEqual(
  topLevelTabsInWorkspace([parentTab, tab("tab-hidden", "hidden-pane", 1)], {
    "parent-pane": pane("parent-pane"),
    "hidden-pane": pane("hidden-pane", undefined, null, true),
  }).map((t) => t.id),
  ["tab-parent"],
  "hidden tab does not surface at top level",
);

console.log("workspaceTabs.test.ts: ok");
