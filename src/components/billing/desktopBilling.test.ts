import { expect, it } from "vitest";
import { parseDesktopCatalog, parseSubscription, scheduledPlanCopy } from "./desktopBilling";

import { desktopCatalogFixture } from "./desktopBilling.testdata";

it("uses provider prices and server eligibility for the monthly Pro trial", () => {
  const result = parseDesktopCatalog(desktopCatalogFixture);
  expect(result.offers[1].prices.month).toMatchObject({ amount: 9.99, priceId: "pri_pro", trialDays: 7, trialEligible: true });
  expect(result.offers[1].prices.year).toMatchObject({ amount: 72, priceId: "pri_pro_year" });
  expect(result.offers[2].features).toContain("20 Neighbors");
  expect(result.offers[2].features).toContain("Unlimited Teams across all workspaces");
  expect(result.changesAvailable).toBe(true);
});

it("refuses malformed or unavailable catalogs instead of manufacturing provider IDs", () => {
  expect(() => parseDesktopCatalog({ status: "unavailable" })).toThrow();
  const malformed = structuredClone(desktopCatalogFixture);
  malformed.catalog.tiers[1].prices.monthly!.amount = "not-money";
  expect(() => parseDesktopCatalog(malformed)).toThrow();
});

it("shows scheduled downgrade dates from persisted server readback", () => {
  const subscription = parseSubscription({ current_tier: "max", period_end: 1700000000,
    pending_change: { to_tier: "pro", effective_at: 1700000000, status: "scheduled" } });
  expect(scheduledPlanCopy(subscription, () => "Oct 25")).toBe("Max until Oct 25, Pro from Oct 25");
  expect(scheduledPlanCopy({ ...subscription, pending_change: { ...subscription.pending_change!, status: "confirmation_pending" } }, () => "Oct 25"))
    .toContain("awaiting confirmation");
  expect(() => parseSubscription({ current_tier: "max", period_end: null, pending_change: { to_tier: "pro", effective_at: -1, status: "scheduled" } })).toThrow();
});
