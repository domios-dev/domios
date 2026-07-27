// Run: npx tsx src/components/Sidebar/workspaceTabs.test.ts

import assert from "node:assert/strict";
import type { Tab, Tab } from "../../lib/types";
import {
  childTabIdsInWorkspace,
  childTabsForParent,
  topLevelTabsInWorkspace,
} from "./workspaceTabs";

function tab(id: string, tabId: string, sortOrder = 0): Tab {
  return { id, tab_id: tabId, sort_order: sortOrder };
}

function tab(
  id: string,
  parentTabId?: string,
  cwd?: string | null,
  hidden?: boolean,
): Tab {
  return {
    id,
    title: id,
    tab_type: "terminal",
    title_level: "Default",
    parent_tab_id: parentTabId ?? null,
    depth: parentTabId ? 1 : 0,
    cwd: cwd ?? null,
    hidden: hidden ?? false,
  } as unknown as Tab;
}

const parentTab = tab("tab-parent", "parent-tab", 0);
const childTab = tab("tab-child", "child-tab", 1);
const movedChildTab = tab("tab-moved", "moved-tab", 2);

assert.deepEqual(
  topLevelTabsInWorkspace([parentTab, childTab], {
    "parent-tab": tab("parent-tab"),
    "child-tab": tab("child-tab", "parent-tab"),
  }).map((t) => t.id),
  ["tab-parent"],
  "in-workspace child nests under parent",
);

assert.deepEqual(
  childTabsForParent([parentTab, childTab], {
    "parent-tab": tab("parent-tab", undefined, "/workspace"),
    "child-tab": tab("child-tab", "parent-tab", "/other/project"),
  }, "parent-tab").map((t) => t.id),
  ["tab-child"],
  "child nests under parent when cwd differs from workspace",
);

assert.deepEqual(
  topLevelTabsInWorkspace([movedChildTab], {
    "moved-tab": tab("moved-tab", "parent-tab"),
  }).map((t) => t.id),
  ["tab-moved"],
  "cross-workspace child surfaces at top level after move-tab",
);

assert.equal(
  childTabIdsInWorkspace([movedChildTab], {
    "moved-tab": tab("moved-tab", "parent-tab"),
  }).size,
  0,
  "cross-workspace child is not treated as nested",
);

const grandparentTab = tab("tab-gp", "gp-tab", 0);
const midTab = tab("tab-mid", "mid-tab", 1);
const leafTab = tab("tab-leaf", "leaf-tab", 2);

assert.deepEqual(
  topLevelTabsInWorkspace([grandparentTab, midTab, leafTab], {
    "gp-tab": tab("gp-tab"),
    "mid-tab": tab("mid-tab", "gp-tab"),
    "leaf-tab": tab("leaf-tab", "mid-tab"),
  }).map((t) => t.id),
  ["tab-gp"],
  "only the hierarchy root is top-level",
);

assert.deepEqual(
  childTabsForParent([grandparentTab, midTab, leafTab], {
    "gp-tab": tab("gp-tab"),
    "mid-tab": tab("mid-tab", "gp-tab"),
    "leaf-tab": tab("leaf-tab", "mid-tab"),
  }, "mid-tab").map((t) => t.id),
  ["tab-leaf"],
  "grandchild renders under its direct parent",
);

const hiddenParentTab = tab("tab-hidden-parent", "hidden-parent", 0);
const visibleChildTab = tab("tab-visible-child", "visible-child", 1);

assert.deepEqual(
  topLevelTabsInWorkspace([hiddenParentTab, visibleChildTab], {
    "hidden-parent": tab("hidden-parent", undefined, null, true),
    "visible-child": tab("visible-child", "hidden-parent"),
  }).map((t) => t.id),
  ["tab-visible-child"],
  "child of hidden parent surfaces at top level",
);

assert.deepEqual(
  topLevelTabsInWorkspace([parentTab, tab("tab-orphan", "missing-tab", 1)], {
    "parent-tab": tab("parent-tab"),
  }).map((t) => t.id),
  ["tab-parent"],
  "tab without a Tab record does not surface at top level",
);

assert.deepEqual(
  topLevelTabsInWorkspace([parentTab, tab("tab-hidden", "hidden-tab", 1)], {
    "parent-tab": tab("parent-tab"),
    "hidden-tab": tab("hidden-tab", undefined, null, true),
  }).map((t) => t.id),
  ["tab-parent"],
  "hidden tab does not surface at top level",
);

console.log("workspaceTabs.test.ts: ok");
