import type { Workspace } from "../../lib/types";

export interface TabNavigationTarget {
  paneId: string;
  workspaceId: string;
}

export function resolveTabNavigationTarget(
  ref: string,
  paneIds: string[],
  workspaces: Workspace[],
): TabNavigationTarget | null {
  const exact = paneIds.find((id) => id === ref);
  const prefixMatches = exact ? [] : paneIds.filter((id) => id.startsWith(ref));
  const paneId = exact ?? (prefixMatches.length === 1 ? prefixMatches[0] : null);
  if (!paneId) return null;

  const workspace = workspaces.find((candidate) =>
    candidate.tabs.some((tab) => tab.pane_id === paneId),
  );
  return workspace ? { paneId, workspaceId: workspace.id } : null;
}
