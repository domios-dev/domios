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
  | { type: "choose"; planKey: string; account: PurchaseAccount; plan: PlanModel | null; catalog: PlanCatalog }
  | { type: "signedIn"; plan: PlanModel | null; catalog: PlanCatalog }
  | { type: "signInFailed" }
  | { type: "cancel" }
  | { type: "checkoutSettled" };

export const IDLE_INTENT: PurchaseIntent = { kind: "idle" };

/** Full access covers every plan; otherwise only an active subscription to the same offer counts. */
export function owns(plan: PlanModel, planKey: string, catalog: PlanCatalog): boolean {
  if (plan.fullAccess) return true;
  return plan.status === "active" && offerIdForPlan(catalog, plan.plan) === offerIdForPlan(catalog, planKey);
}

function decide(planKey: string, plan: PlanModel | null, catalog: PlanCatalog): PurchaseIntent {
  return plan && owns(plan, planKey, catalog) ? { kind: "owned", planKey, staff: plan.fullAccess } : { kind: "opening", planKey };
}

export function purchaseIntent(state: PurchaseIntent, event: PurchaseEvent): PurchaseIntent {
  switch (event.type) {
    case "choose":
      if (state.kind === "signing_in" || state.kind === "opening") return state;
      if (event.account !== "signed_in") return { kind: "signing_in", planKey: event.planKey, attached: event.account === "pending" };
      // Without a known plan ownership is unproven, so checkout never opens; the caller reports it.
      if (!event.plan) return state;
      return decide(event.planKey, event.plan, event.catalog);
    case "signedIn":
      if (state.kind !== "signing_in") return state;
      return event.plan ? decide(state.planKey, event.plan, event.catalog) : IDLE_INTENT;
    case "signInFailed":
      return state.kind === "signing_in" ? IDLE_INTENT : state;
    case "cancel":
      return IDLE_INTENT;
    case "checkoutSettled":
      return state.kind === "opening" ? IDLE_INTENT : state;
  }
}

/** Staff see Pro as their plan wherever plans are shown, even when billing still reads `free`. */
/** The paid plan key of a live provider subscription; team access and cancelled subscriptions have none. */
export function subscribedPlan(plan: PlanModel | null): string | null {
  return plan && !plan.fullAccess && plan.plan !== "free" && plan.status !== "canceled" ? plan.plan : null;
}

export function displayedPlan(plan: PlanModel | null): string | null {
  if (!plan) return null;
  return plan.fullAccess ? "pro" : plan.plan;
}
