import type { Tab } from "../../lib/types";
import { findPlansForTab, ownerIsTabSelf } from "../Canvas/planTabMatch";
import type { Plan, PlannerRef } from "../Canvas/types";

export interface PlanPeerPair {
  planId: string;
  orchestratorTabId: string;
  plannerTabId: string | null;
}

export function resolvePlannerTabIdFromRef(planner: PlannerRef | null | undefined): string | null {
  if (!planner || planner.kind === "waived") return null;
  const id = planner.tab_id?.trim();
  return id || null;
}

export function resolvePlannerTabIdFromPlan(plan: Plan): string | null {
  return resolvePlannerTabIdFromRef(plan.planner);
}

export function findOwnedPlanForOrchestrator(
  orchestratorTabId: string,
  plans: Plan[],
  tabs: Record<string, Tab>,
): Plan | null {
  const sessionId = tabs[orchestratorTabId]?.agent_session_id?.trim() || null;
  const owned = findPlansForTab(plans, orchestratorTabId, tabs).filter((p) =>
    ownerIsTabSelf(p.owner, orchestratorTabId, sessionId),
  );
  if (owned.length === 0) return null;
  return owned.reduce((a, b) => (b.updated_at > a.updated_at ? b : a));
}

export function resolvePlanPeerPair(
  orchestratorTabId: string,
  plans: Plan[],
  tabs: Record<string, Tab>,
): PlanPeerPair | null {
  const plan = findOwnedPlanForOrchestrator(orchestratorTabId, plans, tabs);
  if (!plan) return null;
  return {
    planId: plan.id,
    orchestratorTabId,
    plannerTabId: resolvePlannerTabIdFromPlan(plan),
  };
}

export function isPlannerPeerTab(
  tabId: string,
  orchestratorTabId: string,
  plans: Plan[],
  tabs: Record<string, Tab>,
): boolean {
  const pair = resolvePlanPeerPair(orchestratorTabId, plans, tabs);
  return pair?.plannerTabId === tabId;
}
