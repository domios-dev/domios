import type { Tab } from "../../lib/types";
import { anchorsTeam } from "../../lib/roleKind";
import { isSurfaceTab } from "../../lib/surfaceTab";
import { remoteTeamIdentity } from "./remoteOwnership";
import { sortTeamManagers } from "./orchestrationSections";
import { isSidebarTabVisible } from "./workspaceTabs";

export interface TeamTabMember {
  tab: Tab;
  index: number;
}

export function effectiveTeamId(tab: Tab): string | null {
  return remoteTeamIdentity(tab)?.id ?? tab.team_id ?? null;
}

export function visibleWorkspaceTabs(sortedTabs: readonly Tab[], tabs: Record<string, Tab>): Tab[] {
  const liveTabs = new Map(Object.values(tabs).map((tab) => [tab.id, tab]));
  return sortedTabs.map((tab) => liveTabs.get(tab.id)).filter((tab): tab is Tab => tab != null && isSidebarTabVisible(tab));
}

export function groupWorkspaceTeamMembers(sortedTabs: readonly Tab[]): Map<string, TeamTabMember[]> {
  const grouped = new Map<string, TeamTabMember[]>();
  sortedTabs.forEach((tab, index) => {
    const teamId = effectiveTeamId(tab);
    if (!teamId) return;
    const members = grouped.get(teamId) ?? [];
    members.push({ tab, index });
    grouped.set(teamId, members);
  });
  return grouped;
}

export function partitionTeamMembers(members: readonly TeamTabMember[]) {
  const managers = sortTeamManagers(members.filter((member) => member.tab.role === "Manager"));
  const workers = members.filter((member) => !anchorsTeam(member.tab) && member.tab.role !== "Manager" && member.tab.role != null);
  const agents = members.filter(({ tab }) => tab.role == null && !isSurfaceTab(tab) && (tab.backing?.kind === "chat_backend" || Boolean(tab.agent_type)));
  const agentIds = new Set([...managers, ...workers, ...agents].map((member) => member.tab.id));
  const surfacesByOwner = new Map<string, TeamTabMember[]>();
  const terminals: TeamTabMember[] = [];
  for (const member of members) {
    if (member.tab.role != null || agentIds.has(member.tab.id)) continue;
    const ownerId = member.tab.parent_tab_id;
    if (isSurfaceTab(member.tab) && ownerId && agentIds.has(ownerId)) {
      surfacesByOwner.set(ownerId, [...(surfacesByOwner.get(ownerId) ?? []), member]);
    } else {
      terminals.push(member);
    }
  }
  return { managers, terminals, workers, agents, surfacesByOwner };
}
