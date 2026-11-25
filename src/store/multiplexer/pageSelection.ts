import { isSurfaceTab } from "../../lib/surfaceTab";
import type { MultiplexerState, Tab } from "../../lib/types";

export interface PageSelection {
  active_workspace_id: string | null;
  focused_tab_id: string | null;
  activeTabIds: Record<string, string>;
}

interface SelectionWorkspace {
  id: string;
  tabs: readonly string[];
  active_tab_id?: string | null;
}

export interface SavedPageSelection { workspaceId: string; tabId: string | null }

type SelectionTab = Pick<Tab, "backing" | "hidden" | "show_in_tabbar" | "parent_tab_id">;
type SelectionTabs = Readonly<Record<string, SelectionTab>>;

export interface PageSelectionSnapshot {
  active_workspace_id: string | null;
  workspaces: readonly SelectionWorkspace[];
  tabs: SelectionTabs;
}

export function implicitTabId(ids: readonly string[], tabs: SelectionTabs): string | undefined {
  return ids.find((id) => {
    const tab = tabs[id];
    return tab && !tab.hidden && tab.show_in_tabbar !== false && !isSurfaceTab(tab);
  });
}

export function tabAfterClose(
  closedId: string,
  previousIds: readonly string[],
  previousTabs: SelectionTabs,
  remainingIds: readonly string[],
  remainingTabs: SelectionTabs,
): string | undefined {
  const owner = isSurfaceTab(previousTabs[closedId]) ? previousTabs[closedId].parent_tab_id : null;
  if (owner && remainingIds.includes(owner) && implicitTabId([owner], remainingTabs)) return owner;
  const index = previousIds.indexOf(closedId);
  if (index < 0) return implicitTabId(remainingIds, remainingTabs);
  const remaining = new Set(remainingIds);
  const adjacent = [...previousIds.slice(index + 1), ...previousIds.slice(0, index).reverse()]
    .filter((id) => remaining.has(id));
  return implicitTabId(adjacent, remainingTabs) ?? implicitTabId(remainingIds, remainingTabs);
}

export function initialPageSelection(snapshot: PageSelectionSnapshot, saved: SavedPageSelection | null = null): PageSelection {
  const activeTabIds: Record<string, string> = {};
  for (const workspace of snapshot.workspaces) {
    const selected = workspace.active_tab_id;
    const tabId = selected && workspace.tabs.includes(selected) && snapshot.tabs[selected]
      ? selected : implicitTabId(workspace.tabs, snapshot.tabs);
    if (tabId) activeTabIds[workspace.id] = tabId;
  }
  let workspaceId = snapshot.workspaces.some((workspace) => workspace.id === snapshot.active_workspace_id)
    ? snapshot.active_workspace_id : snapshot.workspaces[0]?.id ?? null;
  const savedWorkspace = snapshot.workspaces.find((workspace) => workspace.id === saved?.workspaceId);
  if (saved && savedWorkspace && (saved.tabId == null || (savedWorkspace.tabs.includes(saved.tabId) && snapshot.tabs[saved.tabId]))) {
    workspaceId = savedWorkspace.id;
    if (saved.tabId) activeTabIds[workspaceId] = saved.tabId;
  }
  return { active_workspace_id: workspaceId, activeTabIds,
    focused_tab_id: workspaceId == null ? null : activeTabIds[workspaceId] ?? null };
}

/** A restored selection seeds the page only until the user navigates; a restore that lands later never replaces a newer choice. */
export function seededSelection(userNavigated: boolean, seed: PageSelection | undefined): PageSelection | undefined {
  return userNavigated ? undefined : seed;
}

export function initialRouteReady(generation: number): boolean {
  return generation > 0;
}

export function shouldRecordHydratedSelection(
  snapshot: MultiplexerState,
  selection: SavedPageSelection,
  pendingCloses: readonly string[],
): boolean {
  const { workspaceId, tabId } = selection;
  const workspace = snapshot.workspaces.find((entry) => entry.id === workspaceId);
  if (!workspace) return false;
  if (tabId == null) return snapshot.active_workspace_id !== workspaceId;
  const tab = snapshot.tabs[tabId];
  if (!tab || tab.remote_peer_id || !workspace.tabs.includes(tabId) || pendingCloses.includes(tabId)) return false;
  return snapshot.active_workspace_id !== workspaceId || workspace.active_tab_id !== tabId
    || (snapshot.focused_tab_id != null && snapshot.focused_tab_id !== tabId);
}

export function reconcilePageSelection(
  previous: PageSelection,
  incoming: PageSelection,
  workspaces: readonly SelectionWorkspace[],
  pageOwned: boolean,
): PageSelection {
  if (!pageOwned) return incoming;
  const activeTabIds = { ...incoming.activeTabIds };
  for (const workspace of workspaces) {
    const tabId = previous.activeTabIds[workspace.id];
    if (tabId && workspace.tabs.includes(tabId)) activeTabIds[workspace.id] = tabId;
  }
  const workspaceId = workspaces.some((workspace) => workspace.id === previous.active_workspace_id)
    ? previous.active_workspace_id : incoming.active_workspace_id;
  return {
    active_workspace_id: workspaceId,
    activeTabIds,
    focused_tab_id: workspaceId == null ? null : activeTabIds[workspaceId] ?? null,
  };
}

export function acceptsPageFocus(
  selection: Pick<PageSelection, "active_workspace_id" | "activeTabIds">,
  tabId: string,
  pageOwned: boolean,
): boolean {
  return !pageOwned || (selection.active_workspace_id != null
    && selection.activeTabIds[selection.active_workspace_id] === tabId);
}
