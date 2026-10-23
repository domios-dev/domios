import type { MultiplexerState } from "../../lib/types";
import { lockedView, tabLockedView } from "./freeLimitModel";
import type { LockBadgeView } from "./LockBadge";

export interface LockedSurfaces {
  workspaces: string[];
  tabs: string[];
  teams: string[];
}

/** Which items each surface marks as locked, read only from the daemon's own `locked` marks. */
export function lockedSurfaces(state: Pick<MultiplexerState, "workspaces" | "tabs" | "teams">): LockedSurfaces {
  const lockedWorkspaces = new Map(state.workspaces.map((workspace) => [workspace.id, workspace.locked === true]));
  const ownerOf = new Map(state.workspaces.flatMap((workspace) => workspace.tabs.map((id) => [id, workspace] as const)));
  return {
    workspaces: state.workspaces.filter((workspace) => lockedView("workspaces", workspace).locked).map((workspace) => workspace.id),
    tabs: Object.values(state.tabs)
      .filter((tab) => tabLockedView(tab, { locked: lockedWorkspaces.get(tab.workspace_id ?? ownerOf.get(tab.id)?.id ?? "") === true }).locked)
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
