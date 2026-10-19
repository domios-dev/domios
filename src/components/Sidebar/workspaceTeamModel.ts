import type { Tab } from "../../lib/types";
import { anchorsTeam, CARTOGRAPHER_KIND, roleKind } from "../../lib/roleKind";
import type { TeamMemberRole } from "../../lib/teamActions";
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

/// Every role a Team with a spawn-capable manager can place.
const TEAM_MEMBER_ROLES = ["orchestrator", "planner", "cartographer", "worker"] as const;
/// With no spawner inside, only an anchoring role can be placed: it takes no
/// parent, so it is the door through which a stalled Team gets one.
const BOOTSTRAP_ONLY_ROLES = ["orchestrator"] as const;
/// A Team holds at most one live Orchestrator, so once it has one the role is
/// not on offer — the daemon refuses it by code either way.
const STAFFED_TEAM_ROLES = ["planner", "cartographer", "worker"] as const;

/// Mirrors the daemon headcount: a removed Orchestrator is gone from the tab
/// list and a dead one has had its agent identity cleared.
function hasLiveOrchestrator(members: readonly TeamTabMember[]): boolean {
  return members.some((member) => anchorsTeam(member.tab) && member.tab.agent_type != null);
}

export function addableTeamRoles(
  members: readonly TeamTabMember[],
  managerTabId: string | null,
): readonly TeamMemberRole[] {
  const hasCartographer = members.some(
    (member) => member.tab.role === "Manager" && roleKind(member.tab) === CARTOGRAPHER_KIND,
  );
  const withoutExistingCartographer = (roles: readonly TeamMemberRole[]): readonly TeamMemberRole[] =>
    hasCartographer ? roles.filter((role) => role !== "cartographer") : roles;
  if (hasLiveOrchestrator(members)) {
    return managerTabId ? withoutExistingCartographer(STAFFED_TEAM_ROLES) : [];
  }
  return managerTabId
    ? withoutExistingCartographer(TEAM_MEMBER_ROLES)
    : BOOTSTRAP_ONLY_ROLES;
}
