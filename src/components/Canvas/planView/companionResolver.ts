import type { OrchestratorRef, PlannerRef } from "../types";

export type CompanionKind = "planner" | "orchestrator";

export interface CompanionResolution {
  kind: CompanionKind;
  paneId: string;
}

export interface CompanionPlanInput {
  planner?: PlannerRef | null;
  owner: OrchestratorRef;
}

export function resolveCompanionPane(
  plan: CompanionPlanInput,
  isPaneLive: (paneId: string) => boolean,
): CompanionResolution | null {
  const planner = plan.planner;
  if (planner && planner.kind === "pane" && isPaneLive(planner.pane_id)) {
    return { kind: "planner", paneId: planner.pane_id };
  }
  const ownerPane = plan.owner?.pane_id ?? null;
  if (ownerPane && isPaneLive(ownerPane)) {
    return { kind: "orchestrator", paneId: ownerPane };
  }
  return null;
}
