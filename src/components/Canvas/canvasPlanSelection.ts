import type { PlanView as Plan } from "./planProjection";
export function selectCanvasPlans(
  livePlans: readonly Plan[],
  fixturePlans: readonly Plan[],
  requestedPlanId?: string,
): Plan[] {
  if (!requestedPlanId) return [...fixturePlans];
  if (fixturePlans.some((plan) => plan.id === requestedPlanId)) return [...fixturePlans];
  return [...livePlans];
}
