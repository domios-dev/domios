import type { Pane, Tab } from "../../lib/types";

function sortByOrder<T extends { sort_order?: number }>(items: T[]): T[] {
  return [...items].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

export function collectPaneIds(tab: Tab, panes?: Record<string, Pane>): string[] {
  if (panes) {
    const p = panes[tab.pane_id];
    return p && !p.hidden ? [tab.pane_id] : [];
  }
  return [tab.pane_id];
}

export function paneIdsInWorkspace(tabs: Tab[], panes: Record<string, Pane>): Set<string> {
  const ids = new Set<string>();
  for (const tab of tabs) {
    for (const pid of collectPaneIds(tab, panes)) {
      ids.add(pid);
    }
  }
  return ids;
}

export function childPaneIdsInWorkspace(tabs: Tab[], panes: Record<string, Pane>): Set<string> {
  const sortedTabs = sortByOrder(tabs);
  const workspacePaneIds = paneIdsInWorkspace(sortedTabs, panes);
  const childPaneIds = new Set<string>();
  for (const tab of sortedTabs) {
    const pids = collectPaneIds(tab, panes);
    const pane = pids[0] ? panes[pids[0]] : null;
    if (pane?.parent_pane_id && workspacePaneIds.has(pane.parent_pane_id)) {
      childPaneIds.add(pids[0]);
    }
  }
  return childPaneIds;
}

export function topLevelTabsInWorkspace(tabs: Tab[], panes: Record<string, Pane>): Tab[] {
  const sortedTabs = sortByOrder(tabs);
  const childPaneIds = childPaneIdsInWorkspace(sortedTabs, panes);
  return sortedTabs.filter((tab) => {
    const pids = collectPaneIds(tab, panes);
    if (pids.length === 0) return false;
    return !childPaneIds.has(pids[0]);
  });
}

export function childTabsForParent(
  tabs: Tab[],
  panes: Record<string, Pane>,
  parentPaneId: string,
): Tab[] {
  return sortByOrder(tabs).filter((tab) => {
    const pids = collectPaneIds(tab, panes);
    const pane = pids[0] ? panes[pids[0]] : null;
    return pane?.parent_pane_id === parentPaneId;
  });
}
