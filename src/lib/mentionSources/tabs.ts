import { canonicalMentionText } from "../../../packages/chat-ui/src/codeterm/canonicalMentions";
import { mentionMatch, type MentionSource } from "../composerMentions";
import { cleanMachineName } from "../machineIdentity";
import type { Tab, Workspace } from "../types";
import { remoteViewerIdentity } from "../remoteTabPortal";
import type { BrowsedWorkspace } from "../../store/multiplexer/pairedWorkspaceProjection";
import { mentionScopeForTab } from "./scope";

export function tabMentionSource(read: () => { tabs: Record<string, Tab>; workspaces: readonly Workspace[]; peerWorkspaces?: readonly BrowsedWorkspace[] }): MentionSource {
  return {
    id: "tabs", group: "Tabs",
    async search(query, { scope, signal }) {
      signal.throwIfAborted();
      if (!scope.workspaceId && !scope.peerId) return [];
      const state = read();
      const { tabs, workspaces, peerWorkspaces } = state;
      return Object.values(tabs).flatMap((tab) => {
        if (tab.tab_type !== "Terminal" || tab.hidden || tab.show_in_tabbar === false) return [];
        const identity = remoteViewerIdentity(tab);
        if (!identity && (tab.remote_peer_id || tab.id.startsWith("mesh:") || scope.peerId)) return [];
        const target = mentionScopeForTab(state, tab.id, peerWorkspaces);
        const crossMachine = (target.peerId ?? null) !== (scope.peerId ?? null);
        const sameWorkspace = target.workspaceId != null && target.workspaceId === scope.workspaceId;
        const otherTeam = target.teamId && target.teamId !== scope.teamId;
        const proximity = crossMachine ? 4 : scope.teamId && target.teamId === scope.teamId ? 0
          : !sameWorkspace ? 3 : otherTeam ? 2 : 1;
        const group = crossMachine ? "Tabs · Mesh" : proximity === 3 ? "Tabs · Other workspaces" : proximity === 0 ? "Tabs · Team"
          : otherTeam ? "Tabs · Other Teams" : "Tabs · Workspace";
        const machineName = identity ? cleanMachineName(tab.remote_peer_name, identity.peerId) ?? "Machine" : undefined;
        const workspaceName = target.peerId
          ? peerWorkspaces?.find(ws => ws.peerId === target.peerId && ws.workspace.id === target.workspaceId)?.workspace.name
          : workspaces.find(ws => ws.id === target.workspaceId)?.name;
        const ids = identity && !crossMachine ? [tab.id, identity.remoteTabId] : [tab.id];
        return [{
          kind: "tab" as const, id: `tabs:${tab.id}`, label: tab.title, group, providerId: tab.agent_type,
          crossMachine,
          machine: identity && crossMachine && machineName ? { id: identity.peerId, name: machineName } : undefined,
          description: [tab.role ?? (tab.agent_type ? "Agent" : "Shell"), tab.description, machineName ?? workspaceName].filter(Boolean).join(" · "),
          insertText: canonicalMentionText("tab", tab.title, tab.id),
          match: mentionMatch(query, [...ids, tab.title, (tab.description ?? "").replace(/\bagent\b/gi, ""), (tab.role ?? "").replace(/\bagent\b/gi, "")]),
          titlePrefix: !!query && tab.title.toLowerCase().startsWith(query.toLowerCase()),
          exact: !!query && [...ids, tab.title].some((value) => value.toLowerCase() === query.toLowerCase()),
          proximity, agentOrder: tab.agent_type || tab.role ? 0 : 1,
        }];
      });
    },
  };
}
