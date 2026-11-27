import type { RemotePeerTabs } from "../../lib/remoteTabPortal";
import type { MultiplexerState, Tab } from "../../lib/types";
import type { BlueprintSnapshot } from "./types";

export function runningBlueprintNamesForOwner(
  blueprints: readonly BlueprintSnapshot[],
  kind: "tab" | "team",
  ownerId: string,
): string[] {
  return blueprints
    .filter((blueprint) => blueprint.state === "running" && blueprint.owner?.kind === kind
      && (kind === "tab" ? blueprint.owner.tab_id : blueprint.owner.team_id) === ownerId)
    .map((blueprint) => blueprint.name ?? blueprint.id);
}

/** Derives each tab's owning workspace from a wire `/api/mux/state` snapshot, where `Tab` itself omits `workspace_id`. */
export function workspaceIdByTabId(
  snapshot: Pick<Partial<MultiplexerState>, "workspaces" | "teams">,
): Map<string, string> {
  const byTab = new Map<string, string>();
  for (const workspace of snapshot.workspaces ?? []) {
    for (const tabId of workspace.tabs ?? []) {
      if (!byTab.has(tabId)) byTab.set(tabId, workspace.id);
    }
  }
  return byTab;
}

/** Populates `workspace_id` on tabs extracted from `/api/mux/state` using workspace and team membership. */
export function hydratePeerTabWorkspaceIds(
  snapshot: Pick<Partial<MultiplexerState>, "workspaces" | "teams">,
  tabs: readonly Tab[],
): Tab[] {
  const byTab = workspaceIdByTabId(snapshot);
  const byTeam = new Map((snapshot.teams ?? []).map((team) => [team.id, team.workspace_id]));
  return tabs.map((tab) => {
    if (tab.workspace_id) return tab;
    const workspaceId = (tab.team_id ? byTeam.get(tab.team_id) : undefined) ?? byTab.get(tab.id);
    return workspaceId ? { ...tab, workspace_id: workspaceId } : tab;
  });
}

export function resolveRemoteTabOwnerWorkspaceId({
  ownerWorkspaceId,
  remoteWorkspaceId,
  peerTabs,
  remoteTabId,
}: {
  ownerWorkspaceId?: string | null;
  remoteWorkspaceId?: string | null;
  peerTabs?: Pick<RemotePeerTabs, "tabs"> | null;
  remoteTabId: string;
}): string | null {
  if (ownerWorkspaceId) return ownerWorkspaceId;
  if (remoteWorkspaceId) return remoteWorkspaceId;
  return peerTabs?.tabs.find((tab) => tab.id === remoteTabId)?.workspace_id ?? null;
}
