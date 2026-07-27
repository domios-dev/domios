import type { Tab, Tab } from "../../lib/types";

function sortByOrder<T extends { sort_order?: number }>(items: T[]): T[] {
  return [...items].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

export function isSidebarTabVisible(tab: Tab, tabs: Record<string, Tab>): boolean {
  const tab = tabs[tab.tab_id];
  return Boolean(tab) && tab.hidden !== true;
}

/** Tab ids that have a tab row in this workspace (regardless of visibility). */
export function tabTabIdsInWorkspace(tabs: Tab[]): Set<string> {
  return new Set(tabs.map((t) => t.tab_id));
}

export function tabIdsInWorkspace(tabs: Tab[], tabs: Record<string, Tab>): Set<string> {
  const ids = new Set<string>();
  for (const tab of tabs) {
    if (isSidebarTabVisible(tab, tabs)) ids.add(tab.tab_id);
  }
  return ids;
}

export function childTabIdsInWorkspace(tabs: Tab[], tabs: Record<string, Tab>): Set<string> {
  const sortedTabs = sortByOrder(tabs);
  const tabTabIds = tabTabIdsInWorkspace(sortedTabs);
  const visibleTabIds = tabIdsInWorkspace(sortedTabs, tabs);
  const childTabIds = new Set<string>();
  for (const tab of sortedTabs) {
    if (!isSidebarTabVisible(tab, tabs)) continue;
    const tab = tabs[tab.tab_id];
    const parentId = tab?.parent_tab_id;
    if (
      parentId &&
      tabTabIds.has(parentId) &&
      visibleTabIds.has(parentId)
    ) {
      childTabIds.add(tab.tab_id);
    }
  }
  return childTabIds;
}

function reachableNestedTabIds(tabs: Tab[], tabs: Record<string, Tab>): Set<string> {
  const sortedTabs = sortByOrder(tabs);
  const childTabIds = childTabIdsInWorkspace(sortedTabs, tabs);
  const reached = new Set<string>();

  const visit = (tab: Tab) => {
    if (reached.has(tab.id)) return;
    reached.add(tab.id);
    const tab = tabs[tab.tab_id];
    if (!tab) return;
    for (const child of childTabsForParent(sortedTabs, tabs, tab.id)) {
      visit(child);
    }
  };

  for (const tab of sortedTabs) {
    if (!isSidebarTabVisible(tab, tabs)) continue;
    if (childTabIds.has(tab.tab_id)) continue;
    visit(tab);
  }
  return reached;
}

export function topLevelTabsInWorkspace(tabs: Tab[], tabs: Record<string, Tab>): Tab[] {
  const sortedTabs = sortByOrder(tabs);
  const childTabIds = childTabIdsInWorkspace(sortedTabs, tabs);
  const reached = reachableNestedTabIds(sortedTabs, tabs);

  return sortedTabs.filter((tab) => {
    if (!isSidebarTabVisible(tab, tabs)) return false;
    if (!childTabIds.has(tab.tab_id)) return true;
    return !reached.has(tab.id);
  });
}

export function childTabsForParent(
  tabs: Tab[],
  tabs: Record<string, Tab>,
  parentTabId: string,
): Tab[] {
  return sortByOrder(tabs).filter((tab) => {
    if (!isSidebarTabVisible(tab, tabs)) return false;
    return tabs[tab.tab_id]?.parent_tab_id === parentTabId;
  });
}

/** Depth-first sidebar tab order (top-level roots, then nested children). */
export function flattenSidebarTabs(tabs: Tab[], tabs: Record<string, Tab>): Tab[] {
  const sortedTabs = sortByOrder(tabs);
  const topLevel = topLevelTabsInWorkspace(sortedTabs, tabs);
  const flat: Tab[] = [];
  const visit = (tab: Tab) => {
    flat.push(tab);
    const tab = tabs[tab.tab_id];
    if (!tab) return;
    for (const child of childTabsForParent(sortedTabs, tabs, tab.id)) {
      visit(child);
    }
  };
  for (const tab of topLevel) visit(tab);
  return flat;
}
