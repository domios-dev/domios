import { roleKindLabel } from "../../lib/roleKind";
import { resolveTabProvider } from "../../lib/tabProvider";
import type { Tab } from "../../lib/types";

type Workspace = import("../../../gen/Workspace").Workspace;
type Team = import("../../../gen/Team").Team;

/** The peer's own workspace and Team names, read from its published state. */
export interface PeerTopology {
  workspaces: Array<Pick<Workspace, "id" | "name"> & Partial<Pick<Workspace, "sort_order" | "tabs">>>;
  teams: Array<Pick<Team, "id" | "workspace_id"> & Partial<Pick<Team, "name">>>;
}

export interface RemoteTabRow { tab: Tab; title: string; kind: string }
/** `id` is null for the workspace's tabs outside any Team. */
export interface RemoteTeamGroup { id: string | null; name: string | null; rows: RemoteTabRow[] }
export interface RemoteWorkspaceGroup { id: string | null; name: string; teams: RemoteTeamGroup[] }

/** A peer's `/api/mux/state` answer reduced to the names the picker shows; anything else reads as unknown. */
export function peerTopologyOf(raw: unknown): PeerTopology | null {
  if (!raw || typeof raw !== "object") return null;
  const state = raw as { workspaces?: unknown; teams?: unknown };
  if (!Array.isArray(state.workspaces)) return null;
  const workspaces = state.workspaces.filter((item): item is PeerTopology["workspaces"][number] =>
    typeof item?.id === "string" && typeof item.name === "string");
  const teams = (Array.isArray(state.teams) ? state.teams : []).filter((item): item is PeerTopology["teams"][number] =>
    typeof item?.id === "string" && typeof item.workspace_id === "string");
  return { workspaces, teams };
}

/** What a row runs, for people: its role, else the agent's provider, else a plain shell. */
export function remoteTabKindLabel(tab: Tab): string {
  const role = roleKindLabel(tab);
  if (role) return role;
  const provider = tab.agent_type || tab.backing?.kind === "chat_backend" ? resolveTabProvider(tab) : null;
  return provider ? `Agent: ${provider.displayName}` : "Shell";
}

const UNKNOWN_ORDER = Number.MAX_SAFE_INTEGER;

/** Visible peer tabs grouped by the peer's workspace, then its Team, in the peer's own order. */
export function groupRemoteTabs(tabs: readonly Tab[], topology: PeerTopology | null): RemoteWorkspaceGroup[] {
  const workspaces = new Map((topology?.workspaces ?? []).map((workspace, index) => [workspace.id, { workspace, index }]));
  const teamNames = new Map((topology?.teams ?? []).map((team) => [team.id, team.name?.trim() || "Team"]));
  const groups = new Map<string, RemoteWorkspaceGroup>();
  for (const tab of tabs) {
    if (tab.hidden || tab.show_in_tabbar === false) continue;
    const workspaceId = tab.workspace_id ?? null;
    const key = workspaceId ?? "";
    let group = groups.get(key);
    if (!group) {
      const known = workspaceId ? workspaces.get(workspaceId)?.workspace : undefined;
      group = { id: workspaceId, name: known?.name || (workspaceId ? "Workspace" : "Other Tabs"), teams: [] };
      groups.set(key, group);
    }
    const teamId = tab.team_id ?? null;
    let team = group.teams.find((candidate) => candidate.id === teamId);
    if (!team) {
      team = { id: teamId, name: teamId ? teamNames.get(teamId) ?? "Team" : null, rows: [] };
      group.teams.push(team);
    }
    team.rows.push({ tab, title: tab.title || tab.id, kind: remoteTabKindLabel(tab) });
  }
  const workspaceRank = (group: RemoteWorkspaceGroup): [number, number] => {
    const known = group.id ? workspaces.get(group.id) : undefined;
    return known ? [known.workspace.sort_order ?? 0, known.index] : [UNKNOWN_ORDER, UNKNOWN_ORDER];
  };
  const ordered = [...groups.values()].sort((left, right) => {
    const [leftOrder, leftIndex] = workspaceRank(left);
    const [rightOrder, rightIndex] = workspaceRank(right);
    return leftOrder - rightOrder || leftIndex - rightIndex;
  });
  for (const group of ordered) {
    const order = group.id ? workspaces.get(group.id)?.workspace.tabs ?? [] : [];
    const position = (row: RemoteTabRow) => {
      const index = order.indexOf(row.tab.id);
      return index < 0 ? UNKNOWN_ORDER : index;
    };
    group.teams.sort((left, right) => Number(left.id !== null) - Number(right.id !== null)
      || (left.name ?? "").localeCompare(right.name ?? ""));
    for (const team of group.teams) team.rows.sort((left, right) => position(left) - position(right));
  }
  return ordered;
}
