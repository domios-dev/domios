import type { ReactNode } from "react";

export type PairAffordanceLayout = "sidebar" | "chips" | "card";

export interface PairAffordanceProps {
  layout: PairAffordanceLayout;
  orchestrator: ReactNode;
  planner: ReactNode;
  className?: string;
  "data-testid"?: string;
}

export function PairAffordance({
  layout,
  orchestrator,
  planner,
  className = "",
  "data-testid": testId = "pair-affordance",
}: PairAffordanceProps) {
  return (
    <div
      data-testid={testId}
      data-pair-affordance-layout={layout}
      className={`pair-affordance pair-affordance--${layout} ${className}`.trim()}
    >
      {layout === "sidebar" ? (
        <>
          <div className="pair-affordance-rail" aria-hidden data-testid="pair-affordance-rail">
            <span className="pair-affordance-rail-arm pair-affordance-rail-arm--top" />
            <span className="pair-affordance-rail-stem" />
            <span className="pair-affordance-rail-arm pair-affordance-rail-arm--bottom" />
          </div>
          <div className="pair-affordance-stack" data-testid="pair-affordance-stack">
            <div className="pair-affordance-slot" data-pair-role="orchestrator">
              {orchestrator}
            </div>
            <div className="pair-affordance-slot" data-pair-role="planner">
              {planner}
            </div>
          </div>
        </>
      ) : (
        <>
          <div className="pair-affordance-slot" data-pair-role="orchestrator">
            {orchestrator}
          </div>
          <span className="pair-affordance-join" aria-hidden data-testid="pair-affordance-join">
            ⟷
          </span>
          <div className="pair-affordance-slot" data-pair-role="planner">
            {planner}
          </div>
        </>
      )}
    </div>
  );
}

export {
  findOwnedPlanForOrchestrator,
  isPlannerPeerPane,
  resolvePlanPeerPair,
  resolvePlannerPaneIdFromPlan,
  resolvePlannerPaneIdFromRef,
  type PlanPeerPair,
} from "./resolvePlanPeerPair";
