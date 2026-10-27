import { describe, expect, it } from "vitest";
import type { PlanModel } from "../Settings/accountPlanModel";
import { BUILTIN_PLAN_CATALOG as catalog } from "./planCatalog";
import { subscribedPlan, IDLE_INTENT, owns, purchaseIntent, type PurchaseEvent, type PurchaseIntent } from "./purchaseIntent";

function plan(overrides: Partial<PlanModel> = {}): PlanModel {
  return { authority: "free", fullAccess: false, plan: "free", status: "active", stale: false, periodEnd: null, cancelAtPeriodEnd: false, limits: null, ...overrides };
}
const FREE = plan();
const PRO = plan({ authority: "billing", plan: "pro_yearly" });
const STAFF = plan({ authority: "admin", fullAccess: true });

const run = (events: PurchaseEvent[], from: PurchaseIntent = IDLE_INTENT) => events.reduce(purchaseIntent, from);
const choose = (planKey: string, account: "signed_in" | "signed_out" | "pending", current: PlanModel | null = null): PurchaseEvent =>
  ({ type: "choose", planKey, account, plan: current, catalog });
const signedIn = (current: PlanModel | null): PurchaseEvent => ({ type: "signedIn", plan: current, catalog });

describe("purchaseIntent", () => {
  it("signed out: choose waits for sign-in, then opens checkout with no second choose", () => {
    const signing = run([choose("pro_yearly", "signed_out")]);
    expect(signing).toEqual({ kind: "signing_in", planKey: "pro_yearly", attached: false });
    expect(run([signedIn(FREE)], signing)).toEqual({ kind: "opening", planKey: "pro_yearly" });
  });

  it("signed in with an unknown plan never opens checkout", () => {
    expect(run([choose("pro_monthly", "signed_in", null)])).toEqual(IDLE_INTENT);
  });

  it("signed in: choose opens checkout at once", () => {
    expect(run([choose("sponsor_yearly", "signed_in", FREE)])).toEqual({ kind: "opening", planKey: "sponsor_yearly" });
  });

  it("an account that already has the plan, in either interval, never reaches checkout", () => {
    expect(run([choose("pro_monthly", "signed_out"), signedIn(PRO)])).toEqual({ kind: "owned", planKey: "pro_monthly", staff: false });
    expect(run([choose("pro_monthly", "signed_in", PRO)])).toEqual({ kind: "owned", planKey: "pro_monthly", staff: false });
  });

  it("staff own every plan and never reach checkout", () => {
    for (const planKey of ["pro_monthly", "pro_yearly", "sponsor_yearly"]) {
      expect(run([choose(planKey, "signed_out"), signedIn(STAFF)])).toEqual({ kind: "owned", planKey, staff: true });
      expect(run([choose(planKey, "signed_in", STAFF)])).toEqual({ kind: "owned", planKey, staff: true });
    }
  });

  it("a Pro subscriber choosing Sponsor still goes to checkout", () => {
    expect(run([choose("sponsor_yearly", "signed_out"), signedIn(PRO)])).toEqual({ kind: "opening", planKey: "sponsor_yearly" });
  });

  it("a lapsed subscription does not count as owning the plan", () => {
    expect(owns(plan({ plan: "pro", status: "canceled" }), "pro_yearly", catalog)).toBe(false);
    expect(owns(plan({ plan: "pro", status: "active" }), "pro_yearly", catalog)).toBe(true);
  });

  it("no checkout without a plan check: an unknown plan after sign-in returns to idle", () => {
    expect(run([choose("pro_yearly", "signed_out"), signedIn(null)])).toEqual(IDLE_INTENT);
  });

  it("cancel drops the intent, and a late sign-in success opens nothing", () => {
    const cancelled = run([choose("pro_yearly", "signed_out"), { type: "cancel" }]);
    expect(cancelled).toEqual(IDLE_INTENT);
    expect(run([signedIn(FREE)], cancelled)).toEqual(IDLE_INTENT);
  });

  it("attaches to a sign-in that is already running instead of starting another", () => {
    const attached = run([choose("pro_yearly", "pending")]);
    expect(attached).toEqual({ kind: "signing_in", planKey: "pro_yearly", attached: true });
    expect(run([signedIn(FREE)], attached)).toEqual({ kind: "opening", planKey: "pro_yearly" });
  });

  it("a failed sign-in returns to idle", () => {
    expect(run([choose("pro_yearly", "signed_out"), { type: "signInFailed" }])).toEqual(IDLE_INTENT);
  });

  it("keeps one intent at a time while signing in or opening", () => {
    const signing = run([choose("pro_yearly", "signed_out")]);
    expect(run([choose("sponsor_yearly", "signed_out")], signing)).toBe(signing);
    const opening = run([choose("pro_yearly", "signed_in", FREE)]);
    expect(run([choose("sponsor_yearly", "signed_in", FREE)], opening)).toBe(opening);
    expect(run([{ type: "checkoutSettled" }], opening)).toEqual(IDLE_INTENT);
  });

  it("from owned, another plan can be chosen", () => {
    const owned = run([choose("pro_yearly", "signed_in", PRO)]);
    expect(run([choose("sponsor_yearly", "signed_in", PRO)], owned)).toEqual({ kind: "opening", planKey: "sponsor_yearly" });
  });

});


describe("subscription status whitelist", () => {
  it("only live provider subscriptions route to management", () => {
    for (const status of ["active", "trialing", "past_due"]) {
      expect(subscribedPlan(plan({ plan: "pro", status }))).toBe("pro");
    }
    for (const status of ["canceled", "revoked", "incomplete", "unpaid", "paused", "unknown", ""]) {
      expect(subscribedPlan(plan({ plan: "pro", status }))).toBeNull();
    }
    expect(subscribedPlan(STAFF)).toBeNull();
    expect(subscribedPlan(FREE)).toBeNull();
    expect(subscribedPlan(null, true)).toBe("already_subscribed");
  });
});
