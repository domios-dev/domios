import { planChangePrice } from "./planCatalog";
import { expect, it } from "vitest";
import { parseDesktopCatalog, parseSubscription } from "./desktopBilling";

import { desktopCatalogFixture } from "./desktopBilling.testdata";

it("uses provider prices and server eligibility for the monthly Pro trial", () => {
  const result = parseDesktopCatalog(desktopCatalogFixture);
  expect(result.offers[1].prices.month).toMatchObject({ amount: 9.99, priceId: "pri_pro", trialDays: 7, trialEligible: true });
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
  const subscription = parseSubscription({ current_tier: "max", period_end: 1700000000,
    pending_change: { to_tier: "pro", effective_at: 1700000000, status: "scheduled" } });
  expect(subscription.pending_change).toEqual({ to_tier: "pro", effective_at: 1700000000, status: "scheduled" });
  expect(parseSubscription({ ...subscription, pending_change: { ...subscription.pending_change!, status: "confirmation_pending" } }).pending_change?.status).toBe("confirmation_pending");
  expect(() => parseSubscription({ current_tier: "max", period_end: null, pending_change: { to_tier: "pro", effective_at: -1, status: "scheduled" } })).toThrow();
});

it("uses the subscription interval for target prices and refuses unknown or missing intervals", () => {
  const catalog = parseDesktopCatalog(desktopCatalogFixture);
  const pro = catalog.offers[1];
  for (const [billing_interval, expected] of [["month", "pri_pro"], ["year", "pri_pro_year"]] as const) {
    const subscription = parseSubscription({ current_tier: "max", billing_interval, period_end: null, pending_change: null });
    expect(planChangePrice(pro, subscription.billing_interval)?.priceId).toBe(expected);
  }
  const withoutInterval = parseSubscription({ current_tier: "max", period_end: null, pending_change: null });
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
