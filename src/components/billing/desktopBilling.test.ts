import { planChangePrice } from "./planCatalog";
import { expect, it } from "vitest";
import { parseDesktopCatalog, parseSubscription } from "./desktopBilling";

import { desktopCatalogFixture } from "./desktopBilling.testdata";

it("uses the provider's no-trial prices and ignores Paddle trial offers", () => {
  const result = parseDesktopCatalog(desktopCatalogFixture);
  expect(result.offers[1].prices.month).toEqual({ amount: 9.99, priceId: "pri_pro", planKey: "pro_monthly", currency: "USD" });
  expect(result.offers[1].prices.year).toMatchObject({ amount: 72, priceId: "pri_pro_year" });
  expect(result.offers[2].limits).toEqual({ "mesh.machines": 12, workspaces: 20, "mem.items": 5000, teams: null, "agent.tabs": null, neighbors: 20 });
  expect(result.changesAvailable).toBe(true);
});

it("refuses malformed or unavailable catalogs instead of manufacturing provider IDs", () => {
  expect(() => parseDesktopCatalog({ status: "unavailable" })).toThrow();
  const malformed = structuredClone(desktopCatalogFixture);
  malformed.catalog.tiers[1].prices.monthly!.amount = "not-money";
  expect(() => parseDesktopCatalog(malformed)).toThrow();
});

it("parses persisted scheduled and confirmation states", () => {
  const subscription = parseSubscription({ current_tier: "max", paid_subscription: null, period_end: 1700000000,
    pending_change: { to_tier: "pro", effective_at: 1700000000, status: "scheduled" } });
  expect(subscription.pending_change).toEqual({ to_tier: "pro", effective_at: 1700000000, status: "scheduled" });
  expect(parseSubscription({ ...subscription, pending_change: { ...subscription.pending_change!, status: "confirmation_pending" } }).pending_change?.status).toBe("confirmation_pending");
  expect(() => parseSubscription({ current_tier: "max", paid_subscription: null, period_end: null, pending_change: { to_tier: "pro", effective_at: -1, status: "scheduled" } })).toThrow();
});

it("uses the subscription interval for target prices and refuses unknown or missing intervals", () => {
  const catalog = parseDesktopCatalog(desktopCatalogFixture);
  const pro = catalog.offers[1];
  for (const [billing_interval, expected] of [["month", "pri_pro"], ["year", "pri_pro_year"]] as const) {
    const subscription = parseSubscription({ current_tier: "max", paid_subscription: null, billing_interval, period_end: null, pending_change: null });
    expect(planChangePrice(pro, subscription.billing_interval)?.priceId).toBe(expected);
  }
  const withoutInterval = parseSubscription({ current_tier: "max", paid_subscription: null, period_end: null, pending_change: null });
  expect(withoutInterval.billing_interval).toBeNull();
  expect(planChangePrice(pro, withoutInterval.billing_interval)).toBeNull();
  expect(planChangePrice(pro, null)).toBeNull();
  expect(planChangePrice({ ...pro, prices: { year: pro.prices.year } }, "month")).toBeNull();
  expect(planChangePrice({ ...pro, prices: { month: { planKey: "opaque", amount: 12, currency: "USD" } } }, "month")).toBeNull();
  expect(() => parseSubscription({ ...withoutInterval, billing_interval: "weekly" })).toThrow();
});

it.each(["test", "live"] as const)("allows changes from an available %s catalog only with a matching interval price", mode => {
  const catalog = parseDesktopCatalog({ ...desktopCatalogFixture, catalog: { ...desktopCatalogFixture.catalog, mode } });
  const pro = catalog.offers[1];
  expect(catalog.changesAvailable).toBe(true);
  expect(planChangePrice(pro, "month")?.priceId).toBe("pri_pro");
  expect(planChangePrice(pro, "year")?.priceId).toBe("pri_pro_year");
  expect(planChangePrice(pro, null)).toBeNull();
  expect(planChangePrice({ ...pro, prices: { year: pro.prices.year } }, "month")).toBeNull();
});

it.each(["free", "paid", "unknown"])("rejects a catalog slot carrying a %s trial instead of opening ordinary checkout", kind => {
  const catalog = structuredClone(desktopCatalogFixture);
  catalog.catalog.prices[0].trial.kind = kind;
  expect(() => parseDesktopCatalog(catalog)).toThrow();
});

it("requires matching provider evidence rather than trusting a no-trial label or slot alone", () => {
  const catalog = structuredClone(desktopCatalogFixture);
  catalog.catalog.prices = [];
  expect(() => parseDesktopCatalog(catalog)).toThrow();
  const mismatch = structuredClone(desktopCatalogFixture);
  mismatch.catalog.prices[0].plan = "max";
  expect(() => parseDesktopCatalog(mismatch)).toThrow();
});

it("keeps effective gifted access separate from an actual Paddle subscription", () => {
  const gifted = { current_tier: "pro", paid_subscription: null, billing_interval: null, period_end: 1700604800, pending_change: null };
  expect(parseSubscription(gifted).paid_subscription).toBeNull();
  const paid_subscription = { tier: "max", status: "active", period_end: 1700604800 };
  expect(parseSubscription({ ...gifted, paid_subscription, billing_interval: "month" }).paid_subscription).toEqual(paid_subscription);
  for (const paid of [undefined, {}, { ...paid_subscription, status: "canceled" }, { ...paid_subscription, tier: "free" }]) {
    expect(() => parseSubscription({ ...gifted, paid_subscription: paid })).toThrow();
  }
});

it("accepts only live paid subscription statuses independently of the effective tier", () => {
  const value = { current_tier: "pro", period_end: 1700604800, pending_change: null,
    paid_subscription: { tier: "max", status: "active", period_end: 1700604800 } };
  for (const status of ["active", "trialing", "past_due"]) {
    expect(parseSubscription({ ...value, paid_subscription: { ...value.paid_subscription, status } }).paid_subscription?.tier).toBe("max");
  }
  for (const status of ["canceled", "revoked", "incomplete", "unpaid", "paused", "unknown", ""]) {
    expect(() => parseSubscription({ ...value, paid_subscription: { ...value.paid_subscription, status } })).toThrow();
  }
});
