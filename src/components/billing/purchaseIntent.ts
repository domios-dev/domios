import type { PlanModel } from "../Settings/accountPlanModel";
import { offerIdForPlan, type PlanCatalog } from "./planCatalog";

/** One chosen plan at a time; `attached` means another surface's sign-in is already running, so this one only waits. */
export type PurchaseIntent =
  | { kind: "idle" }
  | { kind: "signing_in"; planKey: string; attached: boolean }
  | { kind: "opening"; planKey: string }
  | { kind: "owned"; planKey: string; staff: boolean };

export type PurchaseAccount = "signed_in" | "signed_out" | "pending";

export type PurchaseEvent =
  | { type: "choose"; planKey: string; account: PurchaseAccount; plan: PlanModel | null; catalog: PlanCatalog; subscriptionPlan?: string | null }
  | { type: "signedIn"; plan: PlanModel | null; catalog: PlanCatalog; subscriptionPlan?: string | null }
  | { type: "signInFailed" }
  | { type: "cancel" }
  | { type: "checkoutSettled" };

export const IDLE_INTENT: PurchaseIntent = { kind: "idle" };

/** Full access covers every plan; otherwise only an active subscription to the same offer counts. */
export function owns(plan: PlanModel, planKey: string, catalog: PlanCatalog, subscriptionPlan?: string | null): boolean {
  if (plan.fullAccess) return true;
  if (subscriptionPlan !== undefined) return subscriptionPlan !== null && offerIdForPlan(catalog, subscriptionPlan) === offerIdForPlan(catalog, planKey);
  return plan.status === "active" && offerIdForPlan(catalog, plan.plan) === offerIdForPlan(catalog, planKey);
}

function decide(planKey: string, plan: PlanModel | null, catalog: PlanCatalog, subscriptionPlan?: string | null): PurchaseIntent {
  return plan && owns(plan, planKey, catalog, subscriptionPlan) ? { kind: "owned", planKey, staff: plan.fullAccess } : { kind: "opening", planKey };
}

export function purchaseIntent(state: PurchaseIntent, event: PurchaseEvent): PurchaseIntent {
  switch (event.type) {
    case "choose":
      if (state.kind === "signing_in" || state.kind === "opening") return state;
      if (event.account !== "signed_in") return { kind: "signing_in", planKey: event.planKey, attached: event.account === "pending" };
      // Without a known plan ownership is unproven, so checkout never opens; the caller reports it.
      if (!event.plan) return state;
      return decide(event.planKey, event.plan, event.catalog, event.subscriptionPlan);
    case "signedIn":
      if (state.kind !== "signing_in") return state;
      return event.plan ? decide(state.planKey, event.plan, event.catalog, event.subscriptionPlan) : IDLE_INTENT;
    case "signInFailed":
      return state.kind === "signing_in" ? IDLE_INTENT : state;
    case "cancel":
      return IDLE_INTENT;
    case "checkoutSettled":
      return state.kind === "opening" ? IDLE_INTENT : state;
  }
}

/**
 * The paid plan key of a live provider subscription; team access and cancelled subscriptions have none.
 * After the backend refused a checkout as already subscribed, an unknown key routes every paid card to Manage subscription.
 */
export function subscribedPlan(plan: PlanModel | null, refusedAsSubscribed = false): string | null {
  if (plan && !plan.fullAccess && plan.plan !== "free" && ["active", "trialing", "past_due"].includes(plan.status)) return plan.plan;
  return refusedAsSubscribed ? "already_subscribed" : null;
}
