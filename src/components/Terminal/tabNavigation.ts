import type { Workspace } from "../../lib/types";

export interface TabNavigationTarget {
  tabId: string;
  workspaceId: string;
}

export function resolveTabNavigationTarget(
  ref: string,
  tabIds: string[],
  workspaces: Workspace[],
): TabNavigationTarget | null {
  const exact = tabIds.find((id) => id === ref);
  const prefixMatches = exact ? [] : tabIds.filter((id) => id.startsWith(ref));
  const tabId = exact ?? (prefixMatches.length === 1 ? prefixMatches[0] : null);
  if (!tabId) return null;

  const workspace = workspaces.find((candidate) =>
    candidate.tabs.some((tab) => tab.tab_id === tabId),
  );
  return workspace ? { tabId, workspaceId: workspace.id } : null;
}
