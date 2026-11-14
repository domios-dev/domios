import { parseFreeLimits, type FreeLimits } from "../billing/freeLimitModel";

/** The subscription portal could not be reached or answered 503; the action can succeed later unchanged. */
export class SubscriptionServiceUnavailable extends Error {
  constructor(detail: string) {
    super(detail || "Subscription service unavailable");
    this.name = "SubscriptionServiceUnavailable";
  }
}

export interface AccountPlan {
  entitlements: { plan: string; status: string; current_period_end: number; cancel_at_period_end: boolean };
  access: { authority: "admin" | "billing" | "free"; full_access: boolean } | null;
  stale: boolean;
  checking?: boolean;
  verification?: string | null;
  limits?: unknown;
}
/** The server answered paid, but this machine holds no grant it could verify. */
export type PlanVerification = "sign_in_required" | "unconfirmed";
export interface PlanModel {
  authority: "admin" | "billing" | "free" | null;
  fullAccess: boolean;
  plan: string;
  status: string;
  stale: boolean;
  verifying?: boolean;
  unverified?: PlanVerification;
  periodEnd: number | null;
  cancelAtPeriodEnd: boolean;
  limits: FreeLimits | null;
}
function planVerification(value: string | null | undefined): PlanVerification | undefined {
  return value === "sign_in_required" || value === "unconfirmed" ? value : undefined;
}
export function accountPlanModel(value: AccountPlan): PlanModel {
  const unverified = planVerification(value.verification);
  return {authority:value.access?.authority ?? null,fullAccess:value.access?.full_access ?? false,plan:value.entitlements.plan,status:value.entitlements.status,stale:value.stale,verifying:value.checking ?? false,
    ...(unverified ? {unverified} : {}),
    periodEnd:value.entitlements.current_period_end || null,cancelAtPeriodEnd:value.entitlements.cancel_at_period_end,
    limits:parseFreeLimits(value.limits)};
}

export function unverifiedPlanNotice(unverified: PlanVerification | undefined): { label: string; detail: string } | null {
  switch (unverified) {
    case "sign_in_required": return { label: "Sign in again", detail: "Your plan is active, but this machine must sign in again before paid features unlock here." };
    case "unconfirmed": return { label: "Not confirmed", detail: "Your plan is active, but Domios could not confirm it on this machine yet. Paid features stay off until it does." };
    default: return null;
  }
}

export function planFailure(previous: PlanModel | null): PlanModel | null {
  return previous ? {...previous,stale:true} : null;
}

export class NoBillingCustomer extends Error {
  readonly code = "no_billing_customer";

  constructor(message: string) {
    super(message);
    this.name = "NoBillingCustomer";
  }
}
