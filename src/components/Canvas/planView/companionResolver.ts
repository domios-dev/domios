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

// Mirrors src-tauri/src/plan/peer.rs resolve_planner_ref: a live planner binding
// wins; otherwise fall back to the orchestrator owner pane; else nothing live
// resolves (bubble hidden). Keys strictly off THIS plan's bindings — never a
// newest-global pane. Wave-21 upgrade path: prefer planner over orchestrator.
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
