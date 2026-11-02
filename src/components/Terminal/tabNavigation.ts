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

  const owners = workspaces.filter((candidate) => candidate.tabs.includes(tabId));
  return owners.length === 1 ? { tabId, workspaceId: owners[0].id } : null;
}
