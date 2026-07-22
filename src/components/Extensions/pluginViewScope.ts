export interface PluginViewScope {
  workspaceId: string;
  tabId: string;
}

export function pluginViewScope(
  activeWorkspaceId: string | null,
  activeTabIds: Record<string, string | null | undefined>,
): PluginViewScope | null {
  if (!activeWorkspaceId) return null;
  const tabId = activeTabIds[activeWorkspaceId];
  return tabId ? { workspaceId: activeWorkspaceId, tabId } : null;
}

export function pluginViewBelongsToActiveTab(
  owner: PluginViewScope | null,
  active: PluginViewScope | null,
): boolean {
  if (!owner) return true;
  return owner.workspaceId === active?.workspaceId && owner.tabId === active.tabId;
}
