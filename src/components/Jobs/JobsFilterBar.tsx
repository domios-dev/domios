import { useLayoutEffect, useRef, type Ref } from "react";
import { Plus, X } from "lucide-react";
import type { Team } from "../../lib/types";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import type { JobsFilter } from "./jobsModel";
import { addTeamCapsule, addWorkspaceCapsule, removeJobsCapsule, type JobsAxis, type JobsLayout } from "./jobsLayout";

type FocusTarget = "workspace" | "team" | "addWorkspace" | "addTeam";

const CAPSULE = "ct-liquid-interactive inline-flex h-7 min-w-0 max-w-[16rem] items-center gap-1.5 rounded-full border border-[color-mix(in_srgb,var(--ct-accent)_35%,var(--ct-border))] bg-[color-mix(in_srgb,var(--ct-accent)_10%,transparent)] pl-2.5 pr-1.5 text-xs text-[var(--ct-fg)] hover:border-[var(--ct-accent)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]";
const ADD = "ct-liquid-interactive inline-flex h-7 items-center gap-1 rounded-full border border-dashed border-[var(--ct-border)] bg-transparent px-2.5 text-xs text-[var(--ct-muted-fg)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]";
const AXIS_LABEL: Record<JobsAxis, string> = { workspace: "Workspace", team: "Team" };

function Capsule({ axis, value, buttonRef, onRemove }: { axis: JobsAxis; value: string; buttonRef: Ref<HTMLButtonElement>; onRemove: () => void }) {
  const kind = AXIS_LABEL[axis];
  return <button ref={buttonRef} type="button" className={CAPSULE} aria-label={`Remove filter: ${kind} ${value}`} title={`Remove filter: ${kind} ${value}`} onClick={onRemove}>
    <span className="shrink-0 text-[var(--ct-muted-fg)]">{kind}</span>
    <span className="min-w-0 truncate font-medium">{value}</span>
    <X size={12} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
  </button>;
}

/** Workspace and Team capsules; no capsule means every job on this machine. */
export function JobsFilterBar({ filter, layout, teams, onChange }: {
  filter: JobsFilter;
  layout: JobsLayout;
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

  const workspaceItems: DropdownMenuItem[] = layout.workspaceOptions.map((workspace) => ({
    label: workspace.name,
    onClick: () => change(addWorkspaceCapsule(filter, workspace.id, teams), "workspace"),
  }));
  const teamItems: DropdownMenuItem[] = layout.teamOptions.map((team) => ({
    label: team.label,
    description: team.workspaceName ?? undefined,
    onClick: () => change(addTeamCapsule(filter, team.id), "team"),
  }));

  return <div ref={group} role="group" aria-label="Job filters" className="flex min-w-0 flex-wrap items-center gap-1.5">
    {layout.capsules.map((capsule) => <Capsule key={capsule.axis} axis={capsule.axis} value={capsule.value} buttonRef={targets[capsule.axis]}
      onRemove={() => change(removeJobsCapsule(filter, capsule.axis), capsule.axis === "workspace" ? "addWorkspace" : "addTeam")} />)}
    {workspaceItems.length > 0 && <DropdownMenu menuLabel="Add workspace filter" items={workspaceItems} maxHeight={320}
      trigger={<button ref={targets.addWorkspace} type="button" className={ADD} aria-label="Add workspace filter" aria-haspopup="menu"><Plus size={12} aria-hidden="true" />Workspace</button>} />}
    {teamItems.length > 0 && <DropdownMenu menuLabel="Add Team filter" items={teamItems} maxHeight={320}
      trigger={<button ref={targets.addTeam} type="button" className={ADD} aria-label="Add Team filter" aria-haspopup="menu"><Plus size={12} aria-hidden="true" />Team</button>} />}
  </div>;
}
