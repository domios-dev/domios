import type { Pane, Tab } from "../../lib/types";

function sortByOrder<T extends { sort_order?: number }>(items: T[]): T[] {
  return [...items].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

export function isSidebarTabVisible(tab: Tab, panes: Record<string, Pane>): boolean {
  const pane = panes[tab.pane_id];
  return Boolean(pane) && pane.hidden !== true;
}

export function collectPaneIds(tab: Tab, panes?: Record<string, Pane>): string[] {
  if (panes) {
    if (!isSidebarTabVisible(tab, panes)) return [];
    return [tab.pane_id];
  }
  return [tab.pane_id];
}

/** Pane ids that have a tab row in this workspace (regardless of visibility). */
export function tabPaneIdsInWorkspace(tabs: Tab[]): Set<string> {
  return new Set(tabs.map((t) => t.pane_id));
}

export function paneIdsInWorkspace(tabs: Tab[], panes: Record<string, Pane>): Set<string> {
  const ids = new Set<string>();
  for (const tab of tabs) {
    if (isSidebarTabVisible(tab, panes)) ids.add(tab.pane_id);
  }
  return ids;
}

export function childPaneIdsInWorkspace(tabs: Tab[], panes: Record<string, Pane>): Set<string> {
  const sortedTabs = sortByOrder(tabs);
  const tabPaneIds = tabPaneIdsInWorkspace(sortedTabs);
  const visiblePaneIds = paneIdsInWorkspace(sortedTabs, panes);
  const childPaneIds = new Set<string>();
  for (const tab of sortedTabs) {
    if (!isSidebarTabVisible(tab, panes)) continue;
    const pane = panes[tab.pane_id];
    const parentId = pane?.parent_pane_id;
    if (
      parentId &&
      tabPaneIds.has(parentId) &&
      visiblePaneIds.has(parentId)
    ) {
      childPaneIds.add(tab.pane_id);
    }
  }
  return childPaneIds;
}

function reachableNestedTabIds(tabs: Tab[], panes: Record<string, Pane>): Set<string> {
  const sortedTabs = sortByOrder(tabs);
  const childPaneIds = childPaneIdsInWorkspace(sortedTabs, panes);
  const reached = new Set<string>();

  const visit = (tab: Tab) => {
    if (reached.has(tab.id)) return;
    reached.add(tab.id);
    const pane = panes[tab.pane_id];
    if (!pane) return;
    for (const child of childTabsForParent(sortedTabs, panes, pane.id)) {
      visit(child);
    }
  };

  for (const tab of sortedTabs) {
    if (!isSidebarTabVisible(tab, panes)) continue;
    if (childPaneIds.has(tab.pane_id)) continue;
    visit(tab);
  }
  return reached;
}

export function topLevelTabsInWorkspace(tabs: Tab[], panes: Record<string, Pane>): Tab[] {
  const sortedTabs = sortByOrder(tabs);
  const childPaneIds = childPaneIdsInWorkspace(sortedTabs, panes);
  const reached = reachableNestedTabIds(sortedTabs, panes);

  return sortedTabs.filter((tab) => {
    if (!isSidebarTabVisible(tab, panes)) return false;
    if (!childPaneIds.has(tab.pane_id)) return true;
    return !reached.has(tab.id);
  });
}

export function childTabsForParent(
  tabs: Tab[],
  panes: Record<string, Pane>,
  parentPaneId: string,
): Tab[] {
  return sortByOrder(tabs).filter((tab) => {
    if (!isSidebarTabVisible(tab, panes)) return false;
    return panes[tab.pane_id]?.parent_pane_id === parentPaneId;
  });
}

/** Depth-first sidebar tab order (top-level roots, then nested children). */
export function flattenSidebarTabs(tabs: Tab[], panes: Record<string, Pane>): Tab[] {
  const sortedTabs = sortByOrder(tabs);
  const topLevel = topLevelTabsInWorkspace(sortedTabs, panes);
  const flat: Tab[] = [];
  const visit = (tab: Tab) => {
    flat.push(tab);
    const pane = panes[tab.pane_id];
    if (!pane) return;
    for (const child of childTabsForParent(sortedTabs, panes, pane.id)) {
      visit(child);
    }
  };
  for (const tab of topLevel) visit(tab);
  return flat;
}
