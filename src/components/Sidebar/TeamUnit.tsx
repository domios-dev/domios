import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ChevronRight, LogOut, UsersRound } from "lucide-react";
import type { Pane, ProviderDescriptor, Team } from "../../lib/types";
import { TASK_STATUS_COLORS } from "../Canvas/settings";
import type { Plan } from "../Canvas/types";
import { groupTasksIntoColumns } from "../Canvas/planView/boardModel";
import { summarizeWorkers } from "./orchestrationSections";

export type TeamMemberRole = "orchestrator" | "planner" | "worker";

export interface SidebarTeamActions {
  onRenameTeam?: (teamId: string, name: string) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    parentPaneId: string | null,
  ) => void;
  onCloseTeam?: (teamId: string) => void;
}

export interface TeamTaskBucket {
  key: "todo" | "review" | "blocked" | "done";
  label: string;
  count: number;
  color: string;
}

const EMPTY_BUCKETS: TeamTaskBucket[] = [
  { key: "todo", label: "todo", count: 0, color: TASK_STATUS_COLORS.in_progress },
  { key: "review", label: "review", count: 0, color: TASK_STATUS_COLORS.in_review },
  { key: "blocked", label: "blocked", count: 0, color: TASK_STATUS_COLORS.blocked },
  { key: "done", label: "done", count: 0, color: TASK_STATUS_COLORS.done },
];

export function deriveTeamTaskBuckets(
  plan: Plan,
  panes: Record<string, Pane>,
): TeamTaskBucket[] {
  const counts = new Map(
    groupTasksIntoColumns(plan, panes).columns.map((column) => [column.key, column.cards.length]),
  );
  return EMPTY_BUCKETS.map((bucket) => ({
    ...bucket,
    count:
      bucket.key === "todo"
        ? (counts.get("pending") ?? 0) + (counts.get("in_progress") ?? 0)
        : counts.get(bucket.key === "review" ? "in_review" : bucket.key) ?? 0,
  }));
}

export function donutBackground(buckets: readonly TeamTaskBucket[]): string {
  const total = buckets.reduce((sum, bucket) => sum + bucket.count, 0);
  const track = "color-mix(in srgb, var(--ct-fg) 14%, transparent)";
  if (total === 0) return track;

  const active = buckets.filter((bucket) => bucket.count > 0);
  if (active.length === 1) return active[0].color;

  let consumed = 0;
  const segments = active.map((bucket) => {
    const start = (consumed / total) * 100;
    consumed += bucket.count;
    const end = (consumed / total) * 100;
    return `${bucket.color} ${formatPercent(start)}% ${formatPercent(end)}%`;
  });
  return `conic-gradient(${segments.join(", ")})`;
}

function formatPercent(value: number): string {
  return Number(value.toFixed(4)).toString();
}

export function TeamDonut({ buckets }: { buckets: readonly TeamTaskBucket[] }) {
  const total = buckets.reduce((sum, bucket) => sum + bucket.count, 0);
  const done = buckets.find((bucket) => bucket.key === "done")?.count ?? 0;
  return (
    <span
      data-testid="team-donut"
      data-progress={`${done}/${total}`}
      data-buckets={JSON.stringify(buckets)}
      className="sidebar-team-donut"
      style={{ background: donutBackground(buckets) }}
      aria-hidden
    >
      <span className="sidebar-team-donut-hole" />
    </span>
  );
}

interface TeamMenuProps {
  hasPlan: boolean;
  providers: ProviderDescriptor[];
  onRename?: () => void;
  onOpenEffort?: () => void;
  onAddMember?: (role: TeamMemberRole, providerId: string) => void;
  onCloseTeam: () => void;
}

function TeamMenu({
  hasPlan,
  providers,
  onRename,
  onOpenEffort,
  onAddMember,
  onCloseTeam,
}: TeamMenuProps) {
  const [open, setOpen] = useState(false);
  const [providerRole, setProviderRole] = useState<TeamMemberRole | null>(null);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !menuRef.current?.contains(target)) {
        setOpen(false);
        setProviderRole(null);
      }
    };
    document.addEventListener("mousedown", close, true);
    return () => document.removeEventListener("mousedown", close, true);
  }, [open]);

  const run = (action: () => void) => {
    setOpen(false);
    setProviderRole(null);
    action();
  };

  return (
    <div ref={rootRef} className="sidebar-team-menu-root">
      <button
        type="button"
        data-testid="team-menu-trigger"
        aria-label="Team actions"
        aria-expanded={open}
        className="sidebar-team-kebab"
        onClick={(event) => {
          event.stopPropagation();
          const rect = event.currentTarget.getBoundingClientRect();
          setPosition({
            top: Math.min(rect.bottom + 2, window.innerHeight - 8),
            left: Math.max(4, Math.min(rect.right - 174, window.innerWidth - 178)),
          });
          setOpen((value) => !value);
          setProviderRole(null);
        }}
      >
        ⋮
      </button>
      {open && createPortal(
        <div
          ref={menuRef}
          data-testid="team-menu"
          className="sidebar-team-menu"
          style={position}
          onClick={(e) => e.stopPropagation()}
        >
          {providerRole ? (
            <>
              <button type="button" onClick={() => setProviderRole(null)}>
                <ChevronRight size={11} className="rotate-180" /> Back
              </button>
              <div className="sidebar-team-menu-label">Choose agent</div>
              {onAddMember && providers.map((provider) => (
                <button
                  type="button"
                  key={provider.id}
                  onClick={() => run(() => onAddMember(providerRole, provider.id))}
                >
                  {provider.display_name}
                </button>
              ))}
            </>
          ) : (
            <>
              {onRename && <button type="button" onClick={() => run(onRename)}>Rename</button>}
              {hasPlan && onOpenEffort && (
                <button type="button" onClick={() => run(onOpenEffort)}>Open effort</button>
              )}
              {onAddMember && (["orchestrator", "planner", "worker"] as const).map((role) => (
                <button type="button" key={role} onClick={() => setProviderRole(role)}>
                  Add {role} <ChevronRight size={11} className="ml-auto" />
                </button>
              ))}
              <div className="sidebar-team-menu-separator" />
              <button type="button" className="sidebar-team-menu-danger" onClick={() => run(onCloseTeam)}>
                <LogOut size={11} /> Close Team
              </button>
            </>
          )}
        </div>,
        document.body,
      )}
    </div>
  );
}

export interface TeamUnitProps {
  team: Team;
  plan: Plan | null;
  panes: Record<string, Pane>;
  managers: ReactNode;
  managerCount: number;
  workers: ReactNode;
  workerCount: number;
  workingWorkerCount: number;
  providers: ProviderDescriptor[];
  onRenameTeam?: (teamId: string, name: string) => void | Promise<void>;
  onOpenEffort: (target: TeamEffortTarget) => void;
  onAddMember?: (role: TeamMemberRole, providerId: string) => void;
  onCloseTeam: (teamId: string) => void;
}

export interface TeamEffortTarget {
  teamId: string;
  managerPaneId: string | null;
  planId?: string;
}

export function TeamUnit({
  team,
  plan,
  panes,
  managers,
  managerCount,
  workers,
  workerCount,
  workingWorkerCount,
  providers,
  onRenameTeam,
  onOpenEffort,
  onAddMember,
  onCloseTeam,
}: TeamUnitProps) {
  const [expanded, setExpanded] = useState(true);
  const [workersExpanded, setWorkersExpanded] = useState(true);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(team.name?.trim() || "Team");
  const buckets = useMemo(
    () => (plan ? deriveTeamTaskBuckets(plan, panes) : EMPTY_BUCKETS),
    [plan, panes],
  );
  const total = buckets.reduce((sum, bucket) => sum + bucket.count, 0);
  const done = buckets.find((bucket) => bucket.key === "done")?.count ?? 0;

  useEffect(() => setName(team.name?.trim() || "Team"), [team.name]);

  const commitRename = () => {
    const trimmed = name.trim();
    setEditing(false);
    if (trimmed && trimmed !== (team.name?.trim() || "Team") && onRenameTeam) {
      void onRenameTeam(team.id, trimmed);
    }
    else setName(team.name?.trim() || "Team");
  };

  return (
    <section
      data-testid="team-unit"
      data-team-id={team.id}
      data-expanded={expanded ? "true" : "false"}
      className="sidebar-team-unit"
    >
      <div className="sidebar-team-header" onClick={() => setExpanded((value) => !value)}>
        <button
          type="button"
          data-testid="team-collapse-toggle"
          aria-label={expanded ? "Collapse team" : "Expand team"}
          aria-expanded={expanded}
          className="sidebar-team-chevron"
          onClick={(event) => {
            event.stopPropagation();
            setExpanded((value) => !value);
          }}
        >
          <ChevronRight size={11} />
        </button>
        <span data-testid="team-icon" className="sidebar-team-icon" aria-hidden>
          <UsersRound size={14} />
        </span>
        <span className="sidebar-team-title-block">
          {editing ? (
            <input
              autoFocus
              data-testid="team-name-input"
              value={name}
              onChange={(event) => setName(event.target.value)}
              onClick={(event) => event.stopPropagation()}
              onBlur={commitRename}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
                if (event.key === "Escape") {
                  setName(team.name?.trim() || "Team");
                  setEditing(false);
                }
              }}
            />
          ) : (
            <span data-testid="team-name" className="sidebar-team-name">{team.name?.trim() || "Team"}</span>
          )}
        </span>
        {plan && <span className="sidebar-team-count">{done}/{total}</span>}
        <TeamMenu
          hasPlan={Boolean(plan)}
          providers={providers}
          onRename={onRenameTeam ? () => setEditing(true) : undefined}
          onOpenEffort={plan ? () => onOpenEffort({ teamId: team.id, managerPaneId: null, planId: plan.id }) : undefined}
          onAddMember={onAddMember}
          onCloseTeam={() => onCloseTeam(team.id)}
        />
      </div>
      {expanded && (
        <div className="sidebar-team-body">
          {plan ? <>
            <button
              type="button"
              data-testid="team-open-effort"
              className="sidebar-team-effort-row"
              onClick={() => onOpenEffort({ teamId: team.id, managerPaneId: null, planId: plan.id })}
            >
              <TeamDonut buckets={buckets} />
              <span>{plan.title}</span>
              <span className="sidebar-team-effort-action">Open effort</span>
            </button>
            <div data-testid="team-status-readout" className="sidebar-team-readout">
              {buckets.filter((bucket) => bucket.count > 0).map((bucket, index) => (
                <span key={bucket.key} style={{ color: bucket.color }}>
                  {index > 0 && <span className="sidebar-team-readout-dot"> · </span>}
                  {bucket.count} {bucket.label}
                </span>
              ))}
            </div>
          </> : (
            <div data-testid="team-no-effort" className="sidebar-team-no-plan">No effort yet</div>
          )}
          {managerCount > 0 && (
            <div data-testid="managers-section" className="sidebar-team-section">
              <div className="sidebar-team-section-label">MANAGERS {managerCount}</div>
              <div className="sidebar-team-member-list">{managers}</div>
            </div>
          )}
          {workerCount > 0 && (
            <div data-testid="workers-section" className="sidebar-team-section">
              <button
                type="button"
                data-testid="workers-collapse-toggle"
                aria-expanded={workersExpanded}
                className="sidebar-team-workers-toggle"
                onClick={() => setWorkersExpanded((value) => !value)}
              >
                <ChevronRight size={9} /> WORKERS {workerCount}
              </button>
              {workersExpanded ? (
                <div className="sidebar-team-member-list">{workers}</div>
              ) : (
                <div data-testid="workers-collapse-summary" className="sidebar-team-workers-summary">
                  {summarizeWorkers(workerCount, workingWorkerCount)}
                  {workingWorkerCount > 0 && (
                    <span className="sidebar-hierarchy-typing-dots" aria-hidden><span /><span /><span /></span>
                  )}
                  <span className="sidebar-team-summary-kebab">⋮</span>
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
