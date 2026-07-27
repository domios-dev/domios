import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type AnimationEvent as ReactAnimationEvent, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { ChevronRight, ClipboardList, LogOut, UsersRound } from "lucide-react";
import type { Tab, ProviderDescriptor, Team } from "../../lib/types";
import { ProviderGlyph } from "../ProviderGlyph";
import type { Plan } from "../Canvas/types";
import { groupTasksIntoColumns } from "../Canvas/planView/boardModel";
import { summarizeWorkers } from "./orchestrationSections";
import { SidebarDisclosure } from "./SidebarDisclosure";
import { clampFloatingPosition } from "../shared/floatingPosition";
import { ActivityDots } from "../shared/ActivityDots";
import {
  PLAN_STATUS_COLORS,
  SegmentedDonut,
  type SegmentedDonutSegment,
} from "../shared/SegmentedDonut";
import {
  PlanStatusBreakdown,
  type PlanStatusCounts,
} from "../shared/PlanStatusBreakdown";
import { loadTeamDisclosureState, persistTeamDisclosureState } from "./teamDisclosureState";

export type TeamMemberRole = "planner" | "worker";

export interface SidebarTeamActions {
  onRenameTeam?: (teamId: string, name: string, managerTabId: string) => void | Promise<void>;
  onAddTeamMember?: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerTabId: string,
    model?: string,
  ) => void | Promise<void>;
  onCloseTeam?: (teamId: string, managerTabId: string) => void | Promise<void>;
}

export interface TeamTaskBucket {
  key: "todo" | "review" | "blocked" | "done";
  label: string;
  count: number;
  color: string;
}

const EMPTY_BUCKETS: TeamTaskBucket[] = [
  { key: "todo", label: "todo", count: 0, color: PLAN_STATUS_COLORS.todo },
  { key: "review", label: "review", count: 0, color: PLAN_STATUS_COLORS.review },
  { key: "blocked", label: "blocked", count: 0, color: PLAN_STATUS_COLORS.blocked },
  { key: "done", label: "done", count: 0, color: PLAN_STATUS_COLORS.done },
];

export function deriveTeamTaskBuckets(
  plan: Plan,
  tabs: Record<string, Tab>,
): TeamTaskBucket[] {
  const counts = new Map(
    groupTasksIntoColumns(plan, tabs).columns.map((column) => [column.key, column.cards.length]),
  );
  return EMPTY_BUCKETS.map((bucket) => ({
    ...bucket,
    count:
      bucket.key === "todo"
        ? (counts.get("pending") ?? 0) + (counts.get("in_progress") ?? 0)
        : counts.get(bucket.key === "review" ? "in_review" : bucket.key) ?? 0,
  }));
}

export function useTeamPlanMotion(buckets: TeamTaskBucket[]) {
  const signature = buckets.map((bucket) => bucket.count).join(":");
  const total = buckets.reduce((sum, bucket) => sum + bucket.count, 0);
  const complete = total > 0 && buckets.find((bucket) => bucket.key === "done")?.count === total;
  const previous = useRef({ signature, complete, total });
  const [revision, setRevision] = useState(0);
  const [reacting, setReacting] = useState(false);
  const [completionSweep, setCompletionSweep] = useState(false);

  useEffect(() => {
    if (signature === previous.current.signature) return;
    const shouldSweep = previous.current.total > 0 && !previous.current.complete && complete;
    previous.current = { signature, complete, total };
    setRevision((current) => current + 1);
    setReacting(true);
    setCompletionSweep(shouldSweep);
  }, [complete, signature, total]);

  const onAnimationEnd = useCallback((event: ReactAnimationEvent<HTMLElement>) => {
    if (
      event.animationName === "ct-segmented-donut-grow"
      || event.animationName === "sidebar-plan-entry-count-pop"
    ) {
      setReacting(false);
    }
    if (event.animationName === "sidebar-plan-entry-completion-sweep") setCompletionSweep(false);
  }, []);

  return { revision, reacting, completionSweep, onAnimationEnd };
}

interface TeamMenuProps {
  providers: ProviderDescriptor[];
  providersLoading?: boolean;
  contextMenuTargetRef: RefObject<HTMLElement | null>;
  onOpenPlan?: () => void;
  onRename?: () => void;
  onAddMember?: (role: TeamMemberRole, providerId: string, model?: string) => void | Promise<void>;
  onCloseTeam?: () => void;
}

function TeamMenu({
  providers,
  providersLoading = false,
  contextMenuTargetRef,
  onOpenPlan,
  onRename,
  onAddMember,
  onCloseTeam,
}: TeamMenuProps) {
  const [open, setOpen] = useState(false);
  const [providerRole, setProviderRole] = useState<TeamMemberRole | null>(null);
  const [selectedProvider, setSelectedProvider] = useState<ProviderDescriptor | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [addingMember, setAddingMember] = useState(false);
  const [position, setPosition] = useState({ top: 0, left: 0, maxHeight: 0 });
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const providerStepRefs = useRef(new Map<string, HTMLButtonElement>());
  const pendingProviderFocus = useRef<string | null>(null);
  const addingMemberRef = useRef(false);
  const openPointRef = useRef({ x: 0, y: 0 });
  const spawnProviders = providers.filter(
    (provider) => provider.transport === "pty" || provider.transport === "chat_backend",
  );

  const positionMenu = useCallback((x: number, y: number) => {
    const width = menuRef.current?.offsetWidth || 174;
    const height = menuRef.current?.offsetHeight || 220;
    const next = clampFloatingPosition(x, y, width, height, window.innerWidth, window.innerHeight);
    setPosition(next);
  }, []);

  const openAt = useCallback((x: number, y: number) => {
    openPointRef.current = { x, y };
    setProviderRole(null);
    setSelectedProvider(null);
    setActionError(null);
    positionMenu(x, y);
    setOpen(true);
  }, [positionMenu]);

  useEffect(() => {
    const target = contextMenuTargetRef.current;
    if (!target) return;
    const handleContextMenu = (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
      openAt(event.clientX, event.clientY);
    };
    target.addEventListener("contextmenu", handleContextMenu);
    return () => target.removeEventListener("contextmenu", handleContextMenu);
  }, [contextMenuTargetRef, openAt]);

  useLayoutEffect(() => {
    if (!open) return;
    positionMenu(openPointRef.current.x, openPointRef.current.y);
  }, [open, positionMenu, providerRole, selectedProvider]);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !menuRef.current?.contains(target)) {
        setOpen(false);
        setProviderRole(null);
        setSelectedProvider(null);
      }
    };
    document.addEventListener("mousedown", close, true);
    return () => document.removeEventListener("mousedown", close, true);
  }, [open]);

  useEffect(() => {
    const providerId = pendingProviderFocus.current;
    if (selectedProvider || !providerId) return;
    pendingProviderFocus.current = null;
    providerStepRefs.current.get(providerId)?.focus();
  }, [selectedProvider]);

  const run = (action: () => void) => {
    setOpen(false);
    setProviderRole(null);
    setSelectedProvider(null);
    action();
  };

  const addMember = async (role: TeamMemberRole, providerId: string, model?: string) => {
    if (!onAddMember || addingMemberRef.current) return;
    addingMemberRef.current = true;
    setAddingMember(true);
    setActionError(null);
    try {
      await onAddMember(role, providerId, model);
      setOpen(false);
      setProviderRole(null);
      setSelectedProvider(null);
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
        aria-haspopup="menu"
        aria-expanded={open}
        className="sidebar-team-kebab"
        onClick={(event) => {
          event.stopPropagation();
          if (open) {
            setOpen(false);
            return;
          }
          const rect = event.currentTarget.getBoundingClientRect();
          openAt(rect.right - 174, rect.bottom + 2);
        }}
      >
        ⋮
      </button>
      {open && createPortal(
        <div
          ref={menuRef}
          data-testid="team-menu"
          role="menu"
          aria-label="Team actions"
          className="sidebar-team-menu"
          style={{ ...position, overflowY: "auto" }}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              setOpen(false);
              setProviderRole(null);
              setSelectedProvider(null);
              return;
            }
            if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
            const buttons = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
            const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
            const next = (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
            event.preventDefault();
            buttons[next]?.focus();
          }}
        >
          {providerRole && selectedProvider ? (
            <>
              <button type="button" role="menuitem" onClick={() => {
                pendingProviderFocus.current = selectedProvider.id;
                setSelectedProvider(null);
              }}>
                <ChevronRight size={11} className="rotate-180" /> Back
              </button>
              <div className="sidebar-team-menu-label">Choose model</div>
              {selectedProvider.available_models?.length ? selectedProvider.available_models.map((model) => (
                <button
                  type="button"
                  key={model.id}
                  data-testid={`team-picker-model-${model.id}`}
                  role="menuitemradio"
                  aria-checked={model.id === selectedProvider.default_model}
                  disabled={addingMember || model.available === false}
                  onClick={() => void addMember(providerRole, selectedProvider.id, model.id)}
                >
                  {model.display_name}
                </button>
              )) : (
                <div data-testid="team-picker-no-models" role="status">This provider requires a model, but none are available.</div>
              )}
            </>
          ) : providerRole ? (
            <>
              <button type="button" role="menuitem" onClick={() => setProviderRole(null)}>
                <ChevronRight size={11} className="rotate-180" /> Back
              </button>
              <div className="sidebar-team-menu-label">Choose agent</div>
              {providersLoading ? <div data-testid="team-picker-loading" role="status">Loading available providers…</div> : null}
              {!providersLoading && spawnProviders.length === 0 ? <div data-testid="team-picker-empty" role="status">No providers are available to add.</div> : null}
              {onAddMember && !providersLoading && spawnProviders.map((provider) => (
                <button
                  type="button"
                  key={provider.id}
                  ref={(element) => {
                    if (element) providerStepRefs.current.set(provider.id, element);
                  }}
                  disabled={addingMember}
                  role="menuitem"
                  onClick={() => {
                    if ((provider.behavior_config?.model ?? "none") === "none") {
                      void addMember(providerRole, provider.id);
                      return;
                    }
                    setSelectedProvider(provider);
                  }}
                >
                  <ProviderGlyph iconSvg={provider.icon_svg} iconColor={provider.icon_color} iconHint={provider.icon_hint} title={provider.display_name} />
                  {provider.display_name}
                </button>
              ))}
            </>
          ) : (
            <>
              {onOpenPlan && <button type="button" role="menuitem" onClick={() => run(onOpenPlan)}><ClipboardList size={11} /> Open Plan</button>}
              {onRename && <button type="button" role="menuitem" onClick={() => run(onRename)}>Rename</button>}
              {onAddMember && (["planner", "worker"] as const).map((role) => (
                <button type="button" role="menuitem" aria-haspopup="menu" key={role} onClick={() => setProviderRole(role)}>
                  Add {role} <ChevronRight size={11} className="ml-auto" />
                </button>
              ))}
              {onCloseTeam && <>
                <div className="sidebar-team-menu-separator" />
                <button type="button" role="menuitem" className="sidebar-team-menu-danger" onClick={() => run(onCloseTeam)}>
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
  tabs: Record<string, Tab>;
  managers: ReactNode;
  managerCount: number;
  workers: ReactNode;
  workerCount: number;
  workingWorkerCount: number;
  providers: ProviderDescriptor[];
  providersLoading?: boolean;
  onRenameTeam?: (teamId: string, name: string) => void | Promise<void>;
  onOpenEffort: (target: TeamEffortTarget) => void;
  onAddMember?: (role: TeamMemberRole, providerId: string, model?: string) => void | Promise<void>;
  onCloseTeam?: (teamId: string) => void;
}

export interface TeamEffortTarget {
  teamId: string;
  managerTabId: string | null;
  planId?: string;
}

export function TeamUnit({
  team,
  plan,
  tabs,
  managers,
  managerCount,
  workers,
  workerCount,
  workingWorkerCount,
  providers,
  providersLoading,
  onRenameTeam,
  onOpenEffort,
  onAddMember,
  onCloseTeam,
}: TeamUnitProps) {
  const initialDisclosure = useMemo(() => loadTeamDisclosureState(team.id), [team.id]);
  const [expanded, setExpanded] = useState(initialDisclosure.expanded);
  const [workersExpanded, setWorkersExpanded] = useState(initialDisclosure.workersExpanded);
  const [menuDismissed, setMenuDismissed] = useState(false);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(team.name?.trim() || "Team");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const teamTabCount = managerCount + workerCount;
  const displayTitle = plan?.title.trim() || team.name?.trim() || "Team";
  const buckets = useMemo(
    () => (plan ? deriveTeamTaskBuckets(plan, tabs) : EMPTY_BUCKETS),
    [plan, tabs],
  );
  const donutSegments = useMemo<SegmentedDonutSegment[]>(
    () => buckets.map((bucket) => ({
      status: bucket.key,
      count: bucket.count,
      color: bucket.color,
    })),
    [buckets],
  );
  const statusCounts = useMemo<PlanStatusCounts>(
    () => ({
      todo: buckets.find((bucket) => bucket.key === "todo")?.count ?? 0,
      review: buckets.find((bucket) => bucket.key === "review")?.count ?? 0,
      blocked: buckets.find((bucket) => bucket.key === "blocked")?.count ?? 0,
      done: buckets.find((bucket) => bucket.key === "done")?.count ?? 0,
    }),
    [buckets],
  );
  const planMotion = useTeamPlanMotion(buckets);

  useEffect(() => setName(team.name?.trim() || "Team"), [team.name]);

  const headerRef = useRef<HTMLDivElement>(null);

  const toggleExpanded = () => {
    setExpanded((current) => {
      const next = !current;
      persistTeamDisclosureState(team.id, { expanded: next });
      return next;
    });
  };

  const toggleWorkersExpanded = () => {
    setWorkersExpanded((current) => {
      const next = !current;
      persistTeamDisclosureState(team.id, { workersExpanded: next });
      return next;
    });
  };

  useEffect(() => {
    const isInsideHeader = (target: EventTarget | null) => target instanceof Node && headerRef.current?.contains(target);
    const dismissMenu = (event: PointerEvent) => {
      if (!isInsideHeader(event.target)) setMenuDismissed(true);
    };
    const restoreKeyboardReveal = (event: FocusEvent) => {
      if (isInsideHeader(event.target)) setMenuDismissed(false);
    };
    document.addEventListener("pointerdown", dismissMenu, true);
    document.addEventListener("focusin", restoreKeyboardReveal, true);
    return () => {
      document.removeEventListener("pointerdown", dismissMenu, true);
      document.removeEventListener("focusin", restoreKeyboardReveal, true);
    };
  }, []);

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

  const cardContents = <>
    {plan ? (
      <SegmentedDonut
        key={`donut-${planMotion.revision}`}
        data-testid="team-donut"
        segments={donutSegments}
        size={22}
        aria-label={`Task status composition: ${statusCounts.todo} todo, ${statusCounts.review} review, ${statusCounts.blocked} blocked, ${statusCounts.done} done`}
      />
    ) : (
      <span data-testid="team-icon" className="sidebar-team-icon" aria-hidden><UsersRound size={14} /></span>
    )}
    <span className="sidebar-team-title-block">
      {editing ? (
        <input
          autoFocus
          data-testid="team-name-input"
          aria-label="Team name"
          value={name}
          onChange={(event) => setName(event.target.value)}
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
        <span className="sidebar-team-title-row">
          <span data-testid="team-name" className="sidebar-team-name">{displayTitle}</span>
          {workingWorkerCount > 0 && (
            <span
              data-testid="team-working-indicator"
              className="sidebar-team-working-indicator"
              role="status"
              aria-label={`${displayTitle} has working members`}
            ><ActivityDots size="small" tone="muted" /></span>
          )}
        </span>
      )}
      {renameError && <div data-testid="team-rename-error" role="alert">{renameError}</div>}
    </span>
  </>;

  return (
    <section
      data-testid="team-unit"
      data-team-id={team.id}
      data-expanded={expanded ? "true" : "false"}
      className="sidebar-team-unit"
      onAnimationEnd={planMotion.onAnimationEnd}
    >
      <div
        ref={headerRef}
        className={`sidebar-team-header${plan && !editing ? " sidebar-team-header-active" : ""}${planMotion.completionSweep ? " sidebar-plan-entry-completion" : ""}`}
        data-menu-dismissed={menuDismissed ? "true" : undefined}
      >
        {plan && !editing ? (
          <button
            type="button"
            data-testid="team-card-activation"
            className="sidebar-team-card-activation"
            aria-label={`Open ${displayTitle}`}
            onClick={() => onOpenEffort({ teamId: team.id, managerTabId: null, planId: plan.id })}
          >
            {cardContents}
          </button>
        ) : (
          <div className="sidebar-team-card-static">{cardContents}</div>
        )}
        <span
          data-testid="team-progress-slot"
          className="sidebar-team-progress-slot"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
            <TeamMenu
              providers={providers}
              providersLoading={providersLoading}
              contextMenuTargetRef={headerRef}
              onOpenPlan={plan ? () => onOpenEffort({ teamId: team.id, managerTabId: null, planId: plan.id }) : undefined}
              onRename={!plan && onRenameTeam ? () => {
                setRenameError(null);
                setEditing(true);
              } : undefined}
              onAddMember={onAddMember}
              onCloseTeam={onCloseTeam ? () => onCloseTeam(team.id) : undefined}
            />
        </span>
      </div>
      {plan && (
        <PlanStatusBreakdown
          key={`tally-${planMotion.revision}`}
          data-testid="team-status-tally"
          className={`sidebar-team-readout${planMotion.completionSweep ? " sidebar-plan-entry-count-completion" : ""}`}
          counts={statusCounts}
          itemTestIdPrefix="team-status"
        />
      )}
      {expanded && (
        <div className="sidebar-team-body">
          {managerCount > 0 && (
            <div data-testid="managers-section" className="sidebar-team-section">
              <div className="sidebar-team-section-label">MANAGERS {managerCount}</div>
              <div className="sidebar-team-member-list">{managers}</div>
            </div>
          )}
          {workerCount > 0 && (
            <div data-testid="workers-section" className="sidebar-team-section">
              <SidebarDisclosure
                data-testid="workers-collapse-toggle"
                expanded={workersExpanded}
                className="sidebar-team-workers-toggle"
                onClick={toggleWorkersExpanded}
              >WORKERS {workerCount}</SidebarDisclosure>
              {workersExpanded ? (
                <div className="sidebar-team-member-list">{workers}</div>
              ) : (
                <div data-testid="workers-collapse-summary" className="sidebar-team-workers-summary">
                  {summarizeWorkers(workerCount, workingWorkerCount)}
                  {workingWorkerCount > 0 && (
                    <ActivityDots size="small" tone="muted" />
                  )}
                  <span className="sidebar-team-summary-kebab">⋮</span>
                </div>
              )}
            </div>
          )}
        </div>
      )}
      <button
        type="button"
        data-testid="team-collapse-toggle"
        className="sidebar-team-disclosure-bar"
        aria-label={expanded
          ? "Hide Team tabs"
          : `Show Team tabs, ${teamTabCount} ${teamTabCount === 1 ? "tab" : "tabs"}`}
        aria-expanded={expanded}
        onClick={toggleExpanded}
      >
        <span className="sidebar-team-disclosure-glyph" aria-hidden="true" />
        <span className="sidebar-team-disclosure-label" aria-hidden="true">
          {expanded ? "Hide" : "Show"}
        </span>
        {!expanded && (
          <span className="sidebar-team-disclosure-count" aria-hidden="true">
            {teamTabCount} {teamTabCount === 1 ? "tab" : "tabs"}
          </span>
        )}
      </button>
    </section>
  );
}
