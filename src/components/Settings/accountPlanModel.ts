import { parseFreeLimits, type FreeLimits } from "../billing/freeLimitModel";

export interface AccountPlan {
  entitlements: { plan: string; status: string; current_period_end: number; cancel_at_period_end: boolean };
  access: { authority: "admin" | "billing" | "free"; full_access: boolean } | null;
  stale: boolean;
  limits?: unknown;
}
export interface PlanModel {
  authority: "admin" | "billing" | "free" | null;
  fullAccess: boolean;
  plan: string;
  status: string;
  stale: boolean;
  periodEnd: number | null;
  cancelAtPeriodEnd: boolean;
  limits: FreeLimits | null;
}
export function accountPlanModel(value: AccountPlan): PlanModel {
  return {authority:value.access?.authority ?? null,fullAccess:value.access?.full_access ?? false,plan:value.entitlements.plan,status:value.entitlements.status,stale:value.stale,
    periodEnd:value.entitlements.current_period_end || null,cancelAtPeriodEnd:value.entitlements.cancel_at_period_end,
    limits:parseFreeLimits(value.limits)};
}
export function planFailure(previous: PlanModel | null): PlanModel | null {
  return previous ? {...previous,stale:true} : null;
}
