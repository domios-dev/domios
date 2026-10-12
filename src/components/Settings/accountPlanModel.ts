export interface AccountPlan {
  entitlements: { plan: string; status: string; current_period_end: number; cancel_at_period_end: boolean };
  stale: boolean;
}
export interface PlanModel {
  plan: string;
  status: string;
  stale: boolean;
  periodEnd: number | null;
  cancelAtPeriodEnd: boolean;
}
export function accountPlanModel(value: AccountPlan): PlanModel {
  return {plan:value.entitlements.plan,status:value.entitlements.status,stale:value.stale,
    periodEnd:value.entitlements.current_period_end || null,cancelAtPeriodEnd:value.entitlements.cancel_at_period_end};
}
export function planFailure(previous: PlanModel | null): PlanModel | null {
  return previous ? {...previous,stale:true} : null;
}
