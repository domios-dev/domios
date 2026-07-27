import type { Tab } from "../../lib/types";

function sortByOrder(items: Tab[]): Tab[] {
  return [...items].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

export function isSidebarTabVisible(tab: Tab): boolean {
  return tab.hidden !== true && tab.show_in_tabbar !== false;
}

export function tabsInWorkspace(
  tabIds: readonly string[],
  tabs: Record<string, Tab>,
): Tab[] {
  return sortByOrder(
    tabIds
      .map((tabId) => tabs[tabId])
      .filter((tab): tab is Tab => Boolean(tab)),
  );
}

export function tabIdsInWorkspace(tabIds: readonly string[]): Set<string> {
  return new Set(tabIds);
}

export function childTabIdsInWorkspace(
  tabIds: readonly string[],
  tabs: Record<string, Tab>,
): Set<string> {
  const workspaceIds = tabIdsInWorkspace(tabIds);
  const childIds = new Set<string>();
  for (const tab of tabsInWorkspace(tabIds, tabs)) {
    if (
      isSidebarTabVisible(tab) &&
      tab.parent_tab_id &&
      workspaceIds.has(tab.parent_tab_id) &&
      isSidebarTabVisible(tabs[tab.parent_tab_id])
    ) {
      childIds.add(tab.id);
    }
  }
  return childIds;
}

export function childTabsForParent(
  tabIds: readonly string[],
  tabs: Record<string, Tab>,
  parentTabId: string,
): Tab[] {
  return tabsInWorkspace(tabIds, tabs).filter(
    (tab) => isSidebarTabVisible(tab) && tab.parent_tab_id === parentTabId,
  );
}

function reachableNestedTabIds(
  tabIds: readonly string[],
  tabs: Record<string, Tab>,
): Set<string> {
  const children = childTabIdsInWorkspace(tabIds, tabs);
  const reached = new Set<string>();
  const visit = (tab: Tab) => {
    if (reached.has(tab.id)) return;
    reached.add(tab.id);
    for (const child of childTabsForParent(tabIds, tabs, tab.id)) visit(child);
  };
  for (const tab of tabsInWorkspace(tabIds, tabs)) {
    if (!isSidebarTabVisible(tab) || children.has(tab.id)) continue;
    visit(tab);
  }
  return reached;
}

export function topLevelTabsInWorkspace(
  tabIds: readonly string[],
  tabs: Record<string, Tab>,
): Tab[] {
  const children = childTabIdsInWorkspace(tabIds, tabs);
  const reached = reachableNestedTabIds(tabIds, tabs);
  return tabsInWorkspace(tabIds, tabs).filter((tab) => {
    if (!isSidebarTabVisible(tab)) return false;
    return !children.has(tab.id) || !reached.has(tab.id);
  });
}

export function flattenSidebarTabs(
  tabIds: readonly string[],
  tabs: Record<string, Tab>,
): Tab[] {
  const flat: Tab[] = [];
  const visit = (tab: Tab) => {
    flat.push(tab);
    for (const child of childTabsForParent(tabIds, tabs, tab.id)) visit(child);
  };
  for (const tab of topLevelTabsInWorkspace(tabIds, tabs)) visit(tab);
  return flat;
}
