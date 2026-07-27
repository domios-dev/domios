// Run: npx tsx src/components/Sidebar/workspaceTabs.test.ts

import assert from "node:assert/strict";
import type { Tab } from "../../lib/types";
import {
  childTabIdsInWorkspace,
  childTabsForParent,
  topLevelTabsInWorkspace,
} from "./workspaceTabs";

function tab(
  id: string,
  parentTabId?: string,
  cwd?: string | null,
  hidden?: boolean,
  sortOrder = 0,
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
    sort_order: sortOrder,
  } as unknown as Tab;
}

function records(...items: Tab[]): Record<string, Tab> {
  return Object.fromEntries(items.map((item) => [item.id, item]));
}

assert.deepEqual(
  topLevelTabsInWorkspace(
    ["parent-tab", "child-tab"],
    records(tab("parent-tab"), tab("child-tab", "parent-tab")),
  ).map((item) => item.id),
  ["parent-tab"],
  "in-workspace child nests under parent",
);

assert.deepEqual(
  childTabsForParent(
    ["parent-tab", "child-tab"],
    records(tab("parent-tab", undefined, "/workspace"), tab("child-tab", "parent-tab", "/other/project")),
    "parent-tab",
  ).map((item) => item.id),
  ["child-tab"],
  "child nests under parent when cwd differs from workspace",
);

assert.deepEqual(
  topLevelTabsInWorkspace(["moved-tab"], records(tab("moved-tab", "parent-tab"))).map((item) => item.id),
  ["moved-tab"],
  "cross-workspace child surfaces at top level after move",
);

assert.equal(
  childTabIdsInWorkspace(["moved-tab"], records(tab("moved-tab", "parent-tab"))).size,
  0,
  "cross-workspace child is not treated as nested",
);

const hierarchy = records(
  tab("gp-tab"),
  tab("mid-tab", "gp-tab", null, false, 1),
  tab("leaf-tab", "mid-tab", null, false, 2),
);
assert.deepEqual(
  topLevelTabsInWorkspace(["gp-tab", "mid-tab", "leaf-tab"], hierarchy).map((item) => item.id),
  ["gp-tab"],
  "only the hierarchy root is top-level",
);
assert.deepEqual(
  childTabsForParent(["gp-tab", "mid-tab", "leaf-tab"], hierarchy, "mid-tab").map((item) => item.id),
  ["leaf-tab"],
  "grandchild renders under its direct parent",
);

assert.deepEqual(
  topLevelTabsInWorkspace(
    ["hidden-parent", "visible-child"],
    records(tab("hidden-parent", undefined, null, true), tab("visible-child", "hidden-parent")),
  ).map((item) => item.id),
  ["visible-child"],
  "child of hidden parent surfaces at top level",
);

assert.deepEqual(
  topLevelTabsInWorkspace(["parent-tab", "missing-tab"], records(tab("parent-tab"))).map((item) => item.id),
  ["parent-tab"],
  "missing Tab record does not surface",
);

assert.deepEqual(
  topLevelTabsInWorkspace(
    ["parent-tab", "hidden-tab"],
    records(tab("parent-tab"), tab("hidden-tab", undefined, null, true)),
  ).map((item) => item.id),
  ["parent-tab"],
  "hidden tab does not surface",
);

const tabbarHidden = tab("tabbar-hidden");
tabbarHidden.show_in_tabbar = false;
assert.deepEqual(
  topLevelTabsInWorkspace(["tabbar-hidden"], records(tabbarHidden)).map((item) => item.id),
  [],
  "tab with show_in_tabbar=false does not surface",
);

console.log("workspaceTabs.test.ts: ok");
