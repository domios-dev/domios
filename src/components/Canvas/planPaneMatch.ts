import type { Pane, Team, Workspace } from "../../lib/types";
import type { PlanIndexEntry } from "./plansApi";
import { derivePlanWorkspaceScope } from "./planWorkspaceScope";
import { TaskStatus, type OrchestratorRef, type Plan } from "./types";

/** Read-only context that scopes plans to a single workspace. */
export interface PlanWorkspaceContext {
  workspaces?: Workspace[];
  teams?: Team[];
}

export interface PlanFilterOpts extends PlanWorkspaceContext {
  planId?: string;
  /** Team-plan canvas is immutable to workspace-wide plan selection. */
  teamId?: string;
  orchestratorPaneId?: string;
  panes?: Record<string, Pane>;
  /** Current workspace, or `undefined` for unscoped focus/companion contexts.
   *  Required (never optional) so a workspace-mode caller cannot silently omit
   *  scoping and fall back to every plan. */
  workspaceId: string | undefined;
}

/** True when a plan belongs to this workspace via its team binding or its owner
 *  orchestrator pane living in the workspace. */
export function planBelongsToWorkspace(
  plan: Plan,
  workspaceId: string,
  ctx: PlanWorkspaceContext,
): boolean {
  if (plan.team_id) {
    const team = ctx.teams?.find((t) => t.id === plan.team_id);
    if (team && team.workspace_id === workspaceId) return true;
  }
  return derivePlanWorkspaceScope(plan, ctx.workspaces ?? []).ownerWorkspaceId === workspaceId;
}

export function collectPaneChain(
  paneId: string,
  panes: Record<string, Pane>,
): Pane[] {
  const chain: Pane[] = [];
  const seen = new Set<string>();
  let cur: string | null | undefined = paneId;
  while (cur && !seen.has(cur)) {
    seen.add(cur);
    const pane: Pane | undefined = panes[cur];
    if (!pane) break;
    chain.push(pane);
    cur = pane.parent_pane_id;
  }
  return chain;
}

export function resolveOrchestratorPaneId(
  paneId: string,
  panes: Record<string, Pane>,
): string {
  for (const pane of collectPaneChain(paneId, panes)) {
    if (pane.role === "Orchestrator" || pane.can_spawn) return pane.id;
  }
  return paneId;
}

export function ownerIsPaneSelf(
  owner: OrchestratorRef,
  paneId: string,
  sessionId: string | null,
): boolean {
  if (owner.pane_id === paneId) return true;
  return !!sessionId && owner.session_id === sessionId;
}

function ownerMatchesPaneChain(owner: OrchestratorRef, chain: Pane[]): boolean {
  const paneIds = new Set(chain.map((p) => p.id));
  const sessionIds = new Set(
    chain.map((p) => p.agent_session_id).filter((s): s is string => !!s),
  );
  if (owner.pane_id && paneIds.has(owner.pane_id)) return true;
  if (owner.session_id && sessionIds.has(owner.session_id)) return true;
  return false;
}

function planMatchesPaneChain(plan: Plan, chain: Pane[]): boolean {
  return ownerMatchesPaneChain(plan.owner, chain);
}

function taskActivelyAssignedToPane(plan: Plan, paneId: string): boolean {
  return plan.tasks.some(
    (t) =>
      t.status !== TaskStatus.Done &&
      t.status !== TaskStatus.Cancelled &&
      t.assigned_pane_id === paneId,
  );
}

// PlanButton visibility from the lightweight index alone (no full bodies).
export function findPlanIndexForPane(
  entries: PlanIndexEntry[],
  paneId: string,
  panes: Record<string, Pane>,
): PlanIndexEntry[] {
  const active = entries.filter((e) => e.status === "active");
  const chain = collectPaneChain(paneId, panes);
  return active.filter((e) => {
    if (e.assigned_pane_ids.includes(paneId)) return true;
    if (e.owner.pane_id === paneId) return true;
    return ownerMatchesPaneChain(e.owner, chain);
  });
}

export function findPlansForPane(
  plans: Plan[],
  paneId: string,
  panes: Record<string, Pane>,
): Plan[] {
  const active = plans.filter((p) => p.status === "active");
  const chain = collectPaneChain(paneId, panes);
  return active.filter((plan) => {
    if (taskActivelyAssignedToPane(plan, paneId)) return true;
    if (plan.owner.pane_id === paneId) return true;
    return planMatchesPaneChain(plan, chain);
  });
}

export function filterPlansForCanvas(
  plans: Plan[],
  mode: "workspace" | "focus",
  opts: PlanFilterOpts,
): Plan[] {
  const active = plans.filter((p) => p.status === "active");
  if (mode === "workspace") {
    if (opts.teamId) return active.filter((plan) => plan.team_id === opts.teamId);
    // No workspace scope → no board, never every plan (the bug we removed).
    if (!opts.workspaceId) return [];
    return active.filter((p) => planBelongsToWorkspace(p, opts.workspaceId!, opts));
  }
  if (opts.planId) return active.filter((p) => p.id === opts.planId);
  if (opts.orchestratorPaneId && opts.panes) {
    const chain = collectPaneChain(opts.orchestratorPaneId, opts.panes);
    return active.filter((p) => planMatchesPaneChain(p, chain));
  }
  return active;
}

export function findSectionInPlan(plan: Plan, sectionId: string) {
  function walk(sections: Plan["sections"]): Plan["sections"][number] | null {
    for (const s of sections) {
      if (s.id === sectionId) return s;
      const sub = walk(s.subsections);
      if (sub) return sub;
    }
    return null;
  }
  return walk(plan.sections);
}
