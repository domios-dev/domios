import type { Team } from "../../lib/types";
import { NO_JOBS_FILTER, type JobsFilter } from "./jobsModel";

export interface JobsWorkspace {
  id: string;
  name: string;
}

/** Where the view was opened from. It decides only the initial filter, never the layout. */
export type JobsEntry =
  | { kind: "workspace"; workspaceId: string; teamId?: string | null }
  | { kind: "machine" };

export type JobsAxis = "workspace" | "team";

export interface JobsCapsule {
  axis: JobsAxis;
  value: string;
}

export interface JobsTeamOption {
  id: string;
  label: string;
  workspaceName: string | null;
}

export type JobsEmptyScope = "team" | "workspace" | "machine";

export interface JobsLayout {
  capsules: JobsCapsule[];
  workspaceOptions: JobsWorkspace[];
  teamOptions: JobsTeamOption[];
  teamAvailable: boolean;
  cronWorkspace: string | null;
  sources: string[];
  emptyScope: JobsEmptyScope;
  canWiden: boolean;
}

export type JobsOpenScope = "workspace" | "machine";

/** A machine-wide open ignores the opening workspace and Team; the view starts with no capsule. */
export function jobsOpenEntry(workspaceId: string, scope: JobsOpenScope, teamId?: string | null): JobsEntry {
  return scope === "machine" ? { kind: "machine" } : { kind: "workspace", workspaceId, teamId: teamId ?? null };
}

export function jobsEntryFilter(entry: JobsEntry): JobsFilter {
  return entry.kind === "workspace" ? { workspaceId: entry.workspaceId, teamId: entry.teamId ?? null } : NO_JOBS_FILTER;
}

export function teamLabel(team: Team | null): string {
  return team ? team.name?.trim() || team.id : "Unavailable Team";
}

function localTeams(teams: readonly Team[]): Team[] {
  return teams.filter((team) => !team.remote_peer_name);
}

export function jobsLayout(filter: JobsFilter, workspaces: readonly JobsWorkspace[], teams: readonly Team[]): JobsLayout {
  const local = localTeams(teams);
  const workspaceName = (id: string) => workspaces.find((workspace) => workspace.id === id)?.name ?? id;
  const team = filter.teamId === null ? null : local.find((entry) => entry.id === filter.teamId) ?? null;
  const cronWorkspace = filter.workspaceId ?? team?.workspace_id ?? null;
  const capsules: JobsCapsule[] = [
    ...(filter.workspaceId !== null ? [{ axis: "workspace" as const, value: workspaceName(filter.workspaceId) }] : []),
    ...(filter.teamId !== null ? [{ axis: "team" as const, value: teamLabel(team) }] : []),
  ];
  return {
    capsules,
    workspaceOptions: filter.workspaceId === null ? [...workspaces] : [],
    teamOptions: filter.teamId !== null ? [] : local
      .filter((entry) => filter.workspaceId === null || entry.workspace_id === filter.workspaceId)
      .map((entry) => ({ id: entry.id, label: teamLabel(entry), workspaceName: filter.workspaceId === null ? workspaceName(entry.workspace_id) : null })),
    teamAvailable: filter.teamId === null || team !== null,
    cronWorkspace,
    sources: cronWorkspace !== null ? [cronWorkspace] : workspaces.map((workspace) => workspace.id),
    emptyScope: filter.teamId !== null ? "team" : filter.workspaceId !== null ? "workspace" : "machine",
    canWiden: capsules.length > 0,
  };
}

export function removeJobsCapsule(filter: JobsFilter, axis: JobsAxis): JobsFilter {
  return axis === "workspace" ? { ...filter, workspaceId: null } : { ...filter, teamId: null };
}

/** Choosing a workspace drops a Team capsule that belongs to another workspace. */
export function addWorkspaceCapsule(filter: JobsFilter, workspaceId: string, teams: readonly Team[]): JobsFilter {
  const team = filter.teamId === null ? null : localTeams(teams).find((entry) => entry.id === filter.teamId) ?? null;
  return { workspaceId, teamId: team && team.workspace_id !== workspaceId ? null : filter.teamId };
}

export function addTeamCapsule(filter: JobsFilter, teamId: string): JobsFilter {
  return { ...filter, teamId };
}
