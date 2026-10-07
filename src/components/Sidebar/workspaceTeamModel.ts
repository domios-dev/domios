import type { Tab } from "../../lib/types";
import { anchorsTeam } from "../../lib/roleKind";
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
  return sortedTabs.map((tab) => tabs[tab.id]).filter((tab): tab is Tab => Boolean(tab) && isSidebarTabVisible(tab));
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
  return {
    managers: sortTeamManagers(members.filter((member) => member.tab.role === "Manager")),
    terminals: members.filter((member) => member.tab.role == null),
    workers: members.filter((member) => !anchorsTeam(member.tab) && member.tab.role !== "Manager" && member.tab.role != null),
  };
}
