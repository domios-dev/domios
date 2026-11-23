import type { PlanView as Plan } from "../../components/Canvas/planProjection";
import type { MentionScope } from "../composerMentions";
import type { Tab, Workspace } from "../types";
import { remoteViewerIdentity } from "../remoteTabPortal";
import type { BrowsedWorkspace } from "../../store/multiplexer/pairedWorkspaceProjection";

export function mentionScopeForTab(
  state: { tabs: Readonly<Record<string, Tab>>; workspaces: readonly Workspace[] }, tabId: string,
  peerWorkspaces: readonly BrowsedWorkspace[] = [],
): MentionScope {
  const tab = state.tabs[tabId];
  const identity = tab && remoteViewerIdentity(tab);
  if (identity) {
    const owner = peerWorkspaces.find(item => item.peerId === identity.peerId
      && (item.tabs.some(tab => tab.viewerId === tabId) || item.workspace.tabs.includes(identity.remoteTabId)));
    const member = owner?.tabs.find(item => item.viewerId === tabId)?.tab;
    return {
      tabId, peerId: identity.peerId, workspaceId: owner?.workspace.id ?? null,
      projectPath: owner?.workspace.cwd ?? null, cwd: tab.cwd ?? owner?.workspace.cwd ?? null,
      teamId: member ? member.team_id ?? member.ownership?.team?.id ?? null : tab.remote_team_id ?? null,
    };
  }
  const workspace = tab && !tab.remote_peer_id && !tabId.startsWith("mesh:")
    ? state.workspaces.find(item => item.id === tab.workspace_id) ?? state.workspaces.find(item => item.tabs.includes(tabId))
    : undefined;
  return {
    tabId, peerId: null, workspaceId: workspace?.id ?? null,
    projectPath: workspace?.cwd ?? null, cwd: tab?.cwd ?? workspace?.cwd ?? null, teamId: tab?.team_id ?? null,
  };
}

export function projectIdentity(path: string | null | undefined): string | null {
  const normalized = path?.replace(/\\/g, "/").replace(/^\/\/\?\/UNC\//i, "//").replace(/^\/\/\?\//, "").replace(/\/+$/, "") || null;
  return normalized && (/^[a-z]:\//i.test(normalized) || normalized.startsWith("//")) ? normalized.toLowerCase() : normalized;
}

// Missing composer scope must never widen a query to the whole machine.
export function planInMentionScope(plan: Plan, scope: MentionScope): boolean {
  return !!scope.projectPath && projectIdentity(plan.project_path) === projectIdentity(scope.projectPath)
    && plan.status === "active" && plan.mode !== "complete"
    && (plan.team_id ?? null) === scope.teamId;
}
