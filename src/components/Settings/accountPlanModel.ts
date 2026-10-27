import { parseFreeLimits, type FreeLimits } from "../billing/freeLimitModel";

/** The subscription portal could not be reached or answered 503; the action can succeed later unchanged. */
export class SubscriptionServiceUnavailable extends Error {
  constructor(detail: string) {
    super(detail || "Subscription service unavailable");
    this.name = "SubscriptionServiceUnavailable";
  }
}

export interface AccountPlan {
  signed_tier?: string;
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
  return {authority:value.access?.authority ?? null,fullAccess:value.access?.full_access ?? false,plan:value.signed_tier ?? value.entitlements.plan,status:value.entitlements.status,stale:value.stale,
    periodEnd:value.entitlements.current_period_end || null,cancelAtPeriodEnd:value.entitlements.cancel_at_period_end,
    limits:parseFreeLimits(value.limits)};
}

// The portal needs a Paddle customer: billing authority, or any paid record (past due, canceled, or an admin who also pays).
export function manageActionForPlan(plan: PlanModel | null): "portal" | "plans" {
  return plan && (plan.authority === "billing" || plan.plan !== "free") ? "portal" : "plans";
}

export function planFailure(previous: PlanModel | null): PlanModel | null {
  return previous ? {...previous,stale:true} : null;
}
