import type { MultiplexerState, Tab } from "../../lib/types";
import { lockedView, tabLockedView } from "./freeLimitModel";
import type { LockBadgeView } from "./LockBadge";

export interface LockedSurfaces {
  workspaces: string[];
  tabs: string[];
  teams: string[];
}

/** Which items each surface marks as locked, read only from the daemon's own `locked` marks. */
export function lockedSurfaces(state: Pick<MultiplexerState, "workspaces" | "tabs" | "teams">): LockedSurfaces {
  return {
    workspaces: state.workspaces.filter((workspace) => lockedView("workspaces", workspace).locked).map((workspace) => workspace.id),
    tabs: Object.values(state.tabs)
      .filter((tab) => tabLockFromState(state, tab).locked)
      .map((tab) => tab.id),
    teams: (state.teams ?? []).filter((team) => lockedView("teams", team).locked).map((team) => team.id),
  };
}

const MESH_LOCKED: LockBadgeView = { locked: true, label: "Machine over your plan's Mesh machine limit" };
const NOT_LOCKED: LockBadgeView = { locked: false };

/** A Mesh machine row is locked exactly when the account Mesh model marked it over the plan limit. */
export function meshMachineLockedView(row: { status: string }): LockBadgeView {
  return row.status === "over_limit" ? MESH_LOCKED : NOT_LOCKED;
}


export function tabLockFromState(
  state: Pick<MultiplexerState, "workspaces" | "tabs" | "teams">,
  tab: Pick<Tab, "id" | "locked" | "workspace_id" | "team_id"> | null | undefined,
) {
  const workspace = state.workspaces.find((item) => item.id === tab?.workspace_id || item.tabs.includes(tab?.id ?? ""));
  const liveTab = tab?.id ? state.tabs[tab.id] : undefined;
  const teamId = liveTab ? liveTab.team_id : tab?.team_id;
  const team = state.teams?.find((item) => item.id === teamId);
  return tabLockedView(tab, workspace, team);
}
