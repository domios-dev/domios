import type { ReactNode } from "react";

export type PairAffordanceLayout = "chips" | "card";

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
      <div className="pair-affordance-slot" data-pair-role="orchestrator">
        {orchestrator}
      </div>
      <span className="pair-affordance-join" aria-hidden data-testid="pair-affordance-join">
        ⟷
      </span>
      <div className="pair-affordance-slot" data-pair-role="planner">
        {planner}
      </div>
    </div>
  );
}

export {
  findOwnedPlanForOrchestrator,
  isPlannerPeerTab,
  resolvePlanPeerPair,
  resolvePlannerTabIdFromPlan,
  resolvePlannerTabIdFromRef,
  type PlanPeerPair,
} from "./resolvePlanPeerPair";
