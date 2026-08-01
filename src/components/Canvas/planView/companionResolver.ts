import type { OrchestratorRef, PlannerRef } from "../types";

export type CompanionKind = "planner" | "orchestrator";

export interface CompanionResolution {
  kind: CompanionKind;
  tabId: string;
}

export interface CompanionPlanInput {
  planner?: PlannerRef | null;
  owner: OrchestratorRef;
}

export function resolveCompanionTab(
  plan: CompanionPlanInput,
  isTabLive: (tabId: string) => boolean,
): CompanionResolution | null {
  const planner = plan.planner;
  if (planner && planner.kind === "tab" && isTabLive(planner.tab_id)) {
    return { kind: "planner", tabId: planner.tab_id };
  }
  const ownerTab = plan.owner?.tab_id ?? null;
  if (ownerTab && isTabLive(ownerTab)) {
    return { kind: "orchestrator", tabId: ownerTab };
  }
  return null;
}
