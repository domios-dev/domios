interface PageSelection {
  active_workspace_id: string | null;
  focused_tab_id: string | null;
  activeTabIds: Record<string, string>;
}

interface SelectionWorkspace {
  id: string;
  tabs: readonly string[];
  active_tab_id?: string | null;
}

export function initialPageSelection(snapshot: {
  active_workspace_id: string | null;
  workspaces: readonly SelectionWorkspace[];
  tabs: Readonly<Record<string, unknown>>;
}): PageSelection {
  const activeTabIds: Record<string, string> = {};
  for (const workspace of snapshot.workspaces) {
    const selected = workspace.active_tab_id;
    const tabId = selected && workspace.tabs.includes(selected) && snapshot.tabs[selected]
      ? selected : workspace.tabs.find((id) => snapshot.tabs[id]);
    if (tabId) activeTabIds[workspace.id] = tabId;
  }
  const workspaceId = snapshot.workspaces.some((workspace) => workspace.id === snapshot.active_workspace_id)
    ? snapshot.active_workspace_id : snapshot.workspaces[0]?.id ?? null;
  return { active_workspace_id: workspaceId, activeTabIds,
    focused_tab_id: workspaceId == null ? null : activeTabIds[workspaceId] ?? null };
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

export function acceptsPageFocus(
  selection: Pick<PageSelection, "active_workspace_id" | "activeTabIds">,
  tabId: string,
  pageOwned: boolean,
): boolean {
  return !pageOwned || (selection.active_workspace_id != null
    && selection.activeTabIds[selection.active_workspace_id] === tabId);
}
