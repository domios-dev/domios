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

export interface PageSelectionSnapshot {
  active_workspace_id: string | null;
  workspaces: readonly SelectionWorkspace[];
  tabs: Readonly<Record<string, unknown>>;
}

export function initialPageSelection(snapshot: PageSelectionSnapshot, saved: SavedPageSelection | null = null): PageSelection {
  const activeTabIds: Record<string, string> = {};
  for (const workspace of snapshot.workspaces) {
    const selected = workspace.active_tab_id;
    const tabId = selected && workspace.tabs.includes(selected) && snapshot.tabs[selected]
      ? selected : workspace.tabs.find((id) => snapshot.tabs[id]);
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

/** Payload of the daemon's explicit request (CLI, API, agent) to show a tab or workspace in every client. */
export interface FocusRequest { tab_id?: string | null; workspace_id?: string | null }

export const TAB_FOCUS_REQUESTED_EVENT = "tab-focus-requested";

export interface FocusTarget { workspaceId: string; tabId: string | null }

/** What a focus request shows, when this client already knows it; a workspace without a tab keeps this client's own tab there. */
export function focusRequestTarget(
  workspaces: readonly SelectionWorkspace[],
  request: FocusRequest | null | undefined,
): FocusTarget | null {
  const tabId = request?.tab_id ?? null;
  if (!tabId) {
    const workspace = workspaces.find((candidate) => candidate.id === request?.workspace_id);
    return workspace ? { workspaceId: workspace.id, tabId: null } : null;
  }
  const owner = workspaces.find((workspace) => workspace.id === request?.workspace_id && workspace.tabs.includes(tabId))
    ?? workspaces.find((workspace) => workspace.tabs.includes(tabId));
  return owner ? { workspaceId: owner.id, tabId } : null;
}

export function acceptsPageFocus(
  selection: Pick<PageSelection, "active_workspace_id" | "activeTabIds">,
  tabId: string,
  pageOwned: boolean,
): boolean {
  return !pageOwned || (selection.active_workspace_id != null
    && selection.activeTabIds[selection.active_workspace_id] === tabId);
}
