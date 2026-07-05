import type { Pane } from "../../lib/types";
import { findPlansForPane, ownerIsPaneSelf } from "../Canvas/planPaneMatch";
import type { Plan, PlannerRef } from "../Canvas/types";

export interface PlanPeerPair {
  planId: string;
  orchestratorPaneId: string;
  plannerPaneId: string | null;
}

export function resolvePlannerPaneIdFromRef(planner: PlannerRef | null | undefined): string | null {
  if (!planner || planner.kind === "waived") return null;
  const id = planner.pane_id?.trim();
  return id || null;
}

export function resolvePlannerPaneIdFromPlan(plan: Plan): string | null {
  return resolvePlannerPaneIdFromRef(plan.planner);
}

export function findOwnedPlanForOrchestrator(
  orchestratorPaneId: string,
  plans: Plan[],
  panes: Record<string, Pane>,
): Plan | null {
  const sessionId = panes[orchestratorPaneId]?.agent_session_id?.trim() || null;
  const owned = findPlansForPane(plans, orchestratorPaneId, panes).filter((p) =>
    ownerIsPaneSelf(p.owner, orchestratorPaneId, sessionId),
  );
  if (owned.length === 0) return null;
  return owned.reduce((a, b) => (b.updated_at > a.updated_at ? b : a));
}

export function resolvePlanPeerPair(
  orchestratorPaneId: string,
  plans: Plan[],
  panes: Record<string, Pane>,
): PlanPeerPair | null {
  const plan = findOwnedPlanForOrchestrator(orchestratorPaneId, plans, panes);
  if (!plan) return null;
  return {
    planId: plan.id,
    orchestratorPaneId,
    plannerPaneId: resolvePlannerPaneIdFromPlan(plan),
  };
}

export function isPlannerPeerPane(
  paneId: string,
  orchestratorPaneId: string,
  plans: Plan[],
  panes: Record<string, Pane>,
): boolean {
  const pair = resolvePlanPeerPair(orchestratorPaneId, plans, panes);
  return pair?.plannerPaneId === paneId;
}
