import type { Tab, Team, Workspace } from "../../lib/types";
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
  orchestratorTabId?: string;
  tabs?: Record<string, Tab>;
  /** Current workspace, or `undefined` for unscoped focus/companion contexts.
   *  Required (never optional) so a workspace-mode caller cannot silently omit
   *  scoping and fall back to every plan. */
  workspaceId: string | undefined;
}

/** True when a plan belongs to this workspace via its team binding or its owner
 *  orchestrator tab living in the workspace. */
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

export function collectTabChain(
  tabId: string,
  tabs: Record<string, Tab>,
): Tab[] {
  const chain: Tab[] = [];
  const seen = new Set<string>();
  let cur: string | null | undefined = tabId;
  while (cur && !seen.has(cur)) {
    seen.add(cur);
    const tab: Tab | undefined = tabs[cur];
    if (!tab) break;
    chain.push(tab);
    cur = tab.parent_tab_id;
  }
  return chain;
}

export function resolveOrchestratorTabId(
  tabId: string,
  tabs: Record<string, Tab>,
): string {
  for (const tab of collectTabChain(tabId, tabs)) {
    if (tab.role === "Orchestrator" || tab.can_spawn) return tab.id;
  }
  return tabId;
}

export function ownerIsTabSelf(
  owner: OrchestratorRef,
  tabId: string,
  sessionId: string | null,
): boolean {
  if (owner.tab_id === tabId) return true;
  return !!sessionId && owner.session_id === sessionId;
}

function ownerMatchesTabChain(owner: OrchestratorRef, chain: Tab[]): boolean {
  const tabIds = new Set(chain.map((p) => p.id));
  const sessionIds = new Set(
    chain.map((p) => p.agent_session_id).filter((s): s is string => !!s),
  );
  if (owner.tab_id && tabIds.has(owner.tab_id)) return true;
  if (owner.session_id && sessionIds.has(owner.session_id)) return true;
  return false;
}

function planMatchesTabChain(plan: Plan, chain: Tab[]): boolean {
  return ownerMatchesTabChain(plan.owner, chain);
}

function taskActivelyAssignedToTab(plan: Plan, tabId: string): boolean {
  return plan.tasks.some(
    (t) =>
      t.status !== TaskStatus.Done &&
      t.status !== TaskStatus.Cancelled &&
      t.assigned_tab_id === tabId,
  );
}

// PlanButton visibility from the lightweight index alone (no full bodies).
export function findPlanIndexForTab(
  entries: PlanIndexEntry[],
  tabId: string,
  tabs: Record<string, Tab>,
): PlanIndexEntry[] {
  const active = entries.filter((e) => e.status === "active");
  const chain = collectTabChain(tabId, tabs);
  return active.filter((e) => {
    if (e.assigned_tab_ids.includes(tabId)) return true;
    if (e.owner.tab_id === tabId) return true;
    return ownerMatchesTabChain(e.owner, chain);
  });
}

export function findPlansForTab(
  plans: Plan[],
  tabId: string,
  tabs: Record<string, Tab>,
): Plan[] {
  const active = plans.filter((p) => p.status === "active");
  const chain = collectTabChain(tabId, tabs);
  return active.filter((plan) => {
    if (taskActivelyAssignedToTab(plan, tabId)) return true;
    if (plan.owner.tab_id === tabId) return true;
    return planMatchesTabChain(plan, chain);
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
  if (opts.orchestratorTabId && opts.tabs) {
    const chain = collectTabChain(opts.orchestratorTabId, opts.tabs);
    return active.filter((p) => planMatchesTabChain(p, chain));
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
