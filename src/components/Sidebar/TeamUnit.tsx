import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ClipboardList, ChevronRight, LogOut, UsersRound } from "lucide-react";
import type { Pane, ProviderDescriptor, Team } from "../../lib/types";
import { TASK_STATUS_COLORS } from "../Canvas/settings";
import type { Plan } from "../Canvas/types";
import { BOARD_COLUMN_DEFS, groupTasksIntoColumns } from "../Canvas/planView/boardModel";
import { summarizeWorkers } from "./orchestrationSections";

export type TeamMemberRole = "planner" | "worker";

export interface SidebarTeamActions {
  onRenameTeam?: (teamId: string, name: string, managerPaneId: string) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerPaneId: string,
  ) => void | Promise<void>;
  onCloseTeam?: (teamId: string, managerPaneId: string) => void | Promise<void>;
}

export interface TeamTaskBucket {
  key: "todo" | "review" | "blocked" | "done";
  label: string;
  count: number;
  color: string;
}

const statusColorForColumn = (key: (typeof BOARD_COLUMN_DEFS)[number]["key"]): string => {
  const column = BOARD_COLUMN_DEFS.find((definition) => definition.key === key);
  return TASK_STATUS_COLORS[column!.statuses[0]];
};

const EMPTY_BUCKETS: TeamTaskBucket[] = [
  { key: "todo", label: "todo", count: 0, color: statusColorForColumn("in_progress") },
  { key: "review", label: "review", count: 0, color: statusColorForColumn("in_review") },
  { key: "blocked", label: "blocked", count: 0, color: statusColorForColumn("blocked") },
  { key: "done", label: "done", count: 0, color: statusColorForColumn("done") },
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

export function teamDonutBackground(buckets: TeamTaskBucket[]): string {
  const total = buckets.reduce((sum, bucket) => sum + bucket.count, 0);
  if (total === 0) return "color-mix(in srgb, var(--ct-fg) 14%, transparent)";
  let offset = 0;
  const stops = buckets.flatMap((bucket) => {
    if (bucket.count === 0) return [];
    const start = (offset / total) * 100;
    offset += bucket.count;
    return [`${bucket.color} ${start}% ${(offset / total) * 100}%`];
  });
  return `conic-gradient(${stops.join(", ")})`;
}

interface TeamMenuProps {
  providers: ProviderDescriptor[];
  onRename?: () => void;
  onAddMember?: (role: TeamMemberRole, providerId: string) => void | Promise<void>;
  onCloseTeam?: () => void;
}

function TeamMenu({ providers, onRename, onAddMember, onCloseTeam }: TeamMenuProps) {
  const [open, setOpen] = useState(false);
  const [providerRole, setProviderRole] = useState<TeamMemberRole | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [addingMember, setAddingMember] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const addingMemberRef = useRef(false);
  const spawnProviders = providers.filter(
    (provider) => provider.transport === "pty" || provider.transport === "chat_backend",
  );

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

  const addMember = async (role: TeamMemberRole, providerId: string) => {
    if (!onAddMember || addingMemberRef.current) return;
    addingMemberRef.current = true;
    setAddingMember(true);
    setActionError(null);
    try {
      await onAddMember(role, providerId);
      setOpen(false);
      setProviderRole(null);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : "Could not add team member. Try again.");
    } finally {
      addingMemberRef.current = false;
      setAddingMember(false);
    }
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
          setActionError(null);
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
              {onAddMember && spawnProviders.map((provider) => (
                <button
                  type="button"
                  key={provider.id}
                  disabled={addingMember}
                  onClick={() => void addMember(providerRole, provider.id)}
                >
                  {provider.display_name}
                </button>
              ))}
            </>
          ) : (
            <>
              {onRename && <button type="button" onClick={() => run(onRename)}>Rename</button>}
              {onAddMember && (["planner", "worker"] as const).map((role) => (
                <button type="button" key={role} onClick={() => setProviderRole(role)}>
                  Add {role} <ChevronRight size={11} className="ml-auto" />
                </button>
              ))}
              {onCloseTeam && <>
                <div className="sidebar-team-menu-separator" />
                <button type="button" className="sidebar-team-menu-danger" onClick={() => run(onCloseTeam)}>
                  <LogOut size={11} /> Close Team
                </button>
              </>}
            </>
          )}
          {actionError && <div data-testid="team-menu-error" role="alert">{actionError}</div>}
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
  onAddMember?: (role: TeamMemberRole, providerId: string) => void | Promise<void>;
  onCloseTeam?: (teamId: string) => void;
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
  const [renameError, setRenameError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const buckets = useMemo(
    () => (plan ? deriveTeamTaskBuckets(plan, panes) : EMPTY_BUCKETS),
    [plan, panes],
  );

  useEffect(() => setName(team.name?.trim() || "Team"), [team.name]);

  const commitRename = async () => {
    const trimmed = name.trim();
    if (!trimmed || trimmed === (team.name?.trim() || "Team") || !onRenameTeam) {
      setName(team.name?.trim() || "Team");
      setEditing(false);
      return;
    }
    setRenaming(true);
    setRenameError(null);
    try {
      await onRenameTeam(team.id, trimmed);
      setEditing(false);
    } catch (error) {
      setRenameError(error instanceof Error ? error.message : "Could not rename team. Try again.");
    } finally {
      setRenaming(false);
    }
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
        {plan ? (
          <span
            data-testid="team-donut"
            className="sidebar-team-donut"
            aria-label="Task status composition"
            style={{ background: teamDonutBackground(buckets) }}
          ><span data-testid="team-donut-hole" className="sidebar-team-donut-hole" /></span>
        ) : (
          <span data-testid="team-icon" className="sidebar-team-icon" aria-hidden><UsersRound size={14} /></span>
        )}
        <span className="sidebar-team-title-block">
          {editing ? (
            <input
              autoFocus
              data-testid="team-name-input"
              value={name}
              onChange={(event) => setName(event.target.value)}
              onClick={(event) => event.stopPropagation()}
              disabled={renaming}
              onBlur={() => { void commitRename(); }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.currentTarget.blur();
                }
                if (event.key === "Escape") {
                  setName(team.name?.trim() || "Team");
                  setEditing(false);
                }
              }}
            />
          ) : (
            <span data-testid="team-name" className="sidebar-team-name">{team.name?.trim() || "Team"}</span>
          )}
          {plan && <span data-testid="team-effort" className="sidebar-team-effort">{plan.title}</span>}
          {renameError && <div data-testid="team-rename-error" role="alert">{renameError}</div>}
        </span>
        {!expanded && workingWorkerCount > 0 && <span className="sidebar-hierarchy-typing-dots" aria-hidden><span /><span /><span /></span>}
        {plan && <span data-testid="team-done-count" className="sidebar-team-count">{buckets.find((bucket) => bucket.key === "done")?.count ?? 0}/{buckets.reduce((sum, bucket) => sum + bucket.count, 0)}</span>}
        <span className="sidebar-team-controls">
        {plan && (
          <button
            type="button"
            data-testid="team-plan-button"
            aria-label="Open plan"
            title="Open plan"
            className="sidebar-team-kebab"
            onClick={(event) => {
              event.stopPropagation();
              onOpenEffort({ teamId: team.id, managerPaneId: null, planId: plan.id });
            }}
          >
            <ClipboardList size={13} aria-hidden />
          </button>
        )}
        <TeamMenu
          providers={providers}
          onRename={onRenameTeam ? () => {
            setRenameError(null);
            setEditing(true);
          } : undefined}
          onAddMember={onAddMember}
          onCloseTeam={onCloseTeam ? () => onCloseTeam(team.id) : undefined}
        />
        </span>
      </div>
      {expanded && (
        <div className="sidebar-team-body">
          {plan && (
            <span data-testid="team-status-tally" className="sidebar-team-readout">
              {buckets.map((bucket) => (
                <span key={bucket.key} data-testid={`team-status-${bucket.key}`} className="sidebar-team-readout-item" style={{ color: bucket.color }}>
                  <span className="sidebar-team-readout-compact-dot" aria-hidden />
                  <span>{bucket.count}</span><span className="sidebar-team-readout-label"> {bucket.label}</span>
                </span>
              ))}
            </span>
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
