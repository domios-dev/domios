import { useLayoutEffect, useRef, type Ref } from "react";
import { Plus, X } from "lucide-react";
import type { Team } from "../../lib/types";
import { AUTOMATION_TOOL } from "../Automation/styles";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import type { JobsFilter } from "./jobsModel";

export interface JobsWorkspace {
  id: string;
  name: string;
}

type FocusTarget = "workspace" | "team" | "addWorkspace" | "addTeam";

const CAPSULE = "inline-flex h-7 min-w-0 max-w-[16rem] items-center gap-1.5 rounded-full border border-[var(--ct-border)] bg-[var(--ct-bg)] pl-2.5 pr-1.5 text-xs text-[var(--ct-fg)] hover:border-[var(--ct-accent)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]";
const ADD = `${AUTOMATION_TOOL} h-7 rounded-full border-dashed`;

export function teamLabel(team: Team | null): string {
  return team ? team.name?.trim() || team.id : "Unavailable Team";
}

function Capsule({ kind, value, buttonRef, onRemove }: { kind: string; value: string; buttonRef: Ref<HTMLButtonElement>; onRemove: () => void }) {
  return <button ref={buttonRef} type="button" className={CAPSULE} aria-label={`Remove filter: ${kind} ${value}`} title={`Remove filter: ${kind} ${value}`} onClick={onRemove}>
    <span className="shrink-0 text-[var(--ct-muted-fg)]">{kind}</span>
    <span className="min-w-0 truncate font-medium">{value}</span>
    <X size={12} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
  </button>;
}

/** Workspace and Team capsules; no capsule means every job on this machine. */
export function JobsFilterBar({ filter, workspaces, teams, onChange }: {
  filter: JobsFilter;
  workspaces: readonly JobsWorkspace[];
  teams: readonly Team[];
  onChange: (next: JobsFilter) => void;
}) {
  const group = useRef<HTMLDivElement>(null);
  const targets = {
    workspace: useRef<HTMLButtonElement>(null),
    team: useRef<HTMLButtonElement>(null),
    addWorkspace: useRef<HTMLButtonElement>(null),
    addTeam: useRef<HTMLButtonElement>(null),
  };
  const pendingFocus = useRef<FocusTarget | null>(null);

  useLayoutEffect(() => {
    const target = pendingFocus.current;
    if (!target) return;
    pendingFocus.current = null;
    (targets[target].current ?? group.current?.querySelector<HTMLButtonElement>("button"))?.focus();
  });

  const change = (next: JobsFilter, focus: FocusTarget) => {
    pendingFocus.current = focus;
    onChange(next);
  };

  const localTeams = teams.filter((team) => !team.remote_peer_name);
  const workspaceName = (id: string) => workspaces.find((workspace) => workspace.id === id)?.name ?? id;
  const selectedTeam = filter.teamId === null ? null : localTeams.find((team) => team.id === filter.teamId) ?? null;
  const teamOptions = localTeams.filter((team) => filter.workspaceId === null || team.workspace_id === filter.workspaceId);

  const workspaceItems: DropdownMenuItem[] = workspaces.map((workspace) => ({
    label: workspace.name,
    onClick: () => change({ workspaceId: workspace.id, teamId: selectedTeam && selectedTeam.workspace_id !== workspace.id ? null : filter.teamId }, "workspace"),
  }));
  const teamItems: DropdownMenuItem[] = teamOptions.map((team) => ({
    label: teamLabel(team),
    description: filter.workspaceId === null ? workspaceName(team.workspace_id) : undefined,
    onClick: () => change({ ...filter, teamId: team.id }, "team"),
  }));

  return <div ref={group} role="group" aria-label="Job filters" className="flex min-w-0 flex-wrap items-center gap-1.5">
    {filter.workspaceId !== null && <Capsule kind="Workspace" value={workspaceName(filter.workspaceId)} buttonRef={targets.workspace} onRemove={() => change({ ...filter, workspaceId: null }, "addWorkspace")} />}
    {filter.teamId !== null && <Capsule kind="Team" value={teamLabel(selectedTeam)} buttonRef={targets.team} onRemove={() => change({ ...filter, teamId: null }, "addTeam")} />}
    {filter.workspaceId === null && workspaceItems.length > 0 && <DropdownMenu menuLabel="Add workspace filter" items={workspaceItems} maxHeight={320}
      trigger={<button ref={targets.addWorkspace} type="button" className={ADD} aria-label="Add workspace filter" aria-haspopup="menu"><Plus size={12} aria-hidden="true" />Workspace</button>} />}
    {filter.teamId === null && teamItems.length > 0 && <DropdownMenu menuLabel="Add Team filter" items={teamItems} maxHeight={320}
      trigger={<button ref={targets.addTeam} type="button" className={ADD} aria-label="Add Team filter" aria-haspopup="menu"><Plus size={12} aria-hidden="true" />Team</button>} />}
  </div>;
}
