import { mentionMatch, type MentionSource } from "../composerMentions";
import type { Tab, Workspace } from "../types";

export function tabMentionSource(read: () => { tabs: Record<string, Tab>; workspaces: readonly Workspace[] }): MentionSource {
  return {
    id: "tabs", group: "Tabs",
    async search(query, { scope, signal }) {
      signal.throwIfAborted();
      if (!scope.workspaceId) return [];
      const { tabs, workspaces } = read();
      return Object.values(tabs).filter((tab) => tab.tab_type === "Terminal").map((tab) => {
        const workspace = workspaces.find((ws) => ws.id === tab.workspace_id || ws.tabs.includes(tab.id));
        const proximity = tab.remote_peer_id ? 3 : scope.teamId && tab.team_id === scope.teamId ? 0
          : workspace?.id === scope.workspaceId ? 1 : 2;
        const group = proximity === 3 ? "Tabs · Mesh" : proximity === 2 ? "Tabs · Other workspaces" : proximity === 0 ? "Tabs · Team" : "Tabs · Workspace";
        return {
          id: `tabs:${tab.id}`, label: tab.title, group,
          description: [tab.role ?? (tab.agent_type ? "Agent" : "Shell"), tab.description, tab.remote_peer_name ?? workspace?.name].filter(Boolean).join(" · "),
          insertText: `@${tab.title} (tab ${tab.id})`,
          match: mentionMatch(query, [tab.id, tab.title, tab.description ?? "", tab.role ?? ""]),
          exact: !!query && [tab.id, tab.title].some((value) => value.toLowerCase() === query.toLowerCase()),
          proximity, agentOrder: tab.agent_type || tab.role ? 0 : 1,
        };
      });
    },
  };
}
