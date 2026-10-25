import assert from "node:assert/strict";
import {
  BUILTIN_PLAN_CATALOG,
  bestYearlySavings,
  offerIdForPlan,
  offerPrice,
  parsePlanCatalog,
  planCardAction,
  remoteCatalogSource,
  upgradePlanKey,
  type PlanOffer,
} from "./planCatalog";

const offer = (prices: PlanOffer["prices"]): PlanOffer => ({ id: "pro", name: "Pro", tagline: "", features: [], prices });
const usd = (planKey: string, amount: number) => ({ planKey, amount, currency: "USD" });
const pro = offer({ month: usd("pro_monthly", 10), year: usd("pro_yearly", 60) });

assert.deepEqual(offerPrice(pro, "year"), { headline: "$5", per: "/ month", note: "Billed $60 yearly, save 50%", planKey: "pro_yearly" });
assert.deepEqual(offerPrice(pro, "month"), { headline: "$10", per: "/ month", note: "Billed monthly", planKey: "pro_monthly" });

const yearlyOnly = offer({ year: usd("y", 120) });
assert.equal(offerPrice(yearlyOnly, "month").note, "Billed $120 yearly, yearly only");
assert.equal(offerPrice(yearlyOnly, "month").planKey, "y");

assert.equal(offerPrice(offer({}), "year").planKey, null);
assert.equal(offerPrice(offer({ month: usd("m", 5), year: usd("y", 33.6) }), "year").headline, "$2.80");

assert.equal(bestYearlySavings({ offers: [pro, yearlyOnly] }), 50);
assert.equal(bestYearlySavings({ offers: [offer({ month: usd("m", 5), year: { planKey: "y", amount: 30, currency: "EUR" } })] }), null);

assert.equal(offerIdForPlan(BUILTIN_PLAN_CATALOG, null), "free");
assert.equal(offerIdForPlan(BUILTIN_PLAN_CATALOG, "pro_yearly"), "pro");
assert.equal(offerIdForPlan(BUILTIN_PLAN_CATALOG, "sponsor"), "sponsor");

assert.deepEqual(
  BUILTIN_PLAN_CATALOG.offers.flatMap((offer) => Object.values(offer.prices).map((price) => price?.planKey)),
  ["pro_monthly", "pro_yearly", "sponsor_monthly", "sponsor_yearly"],
);

const sponsor = BUILTIN_PLAN_CATALOG.offers.find((item) => item.id === "sponsor")!;
assert.deepEqual(offerPrice(sponsor, "month"), { headline: "$29.99", per: "/ month", note: "Billed monthly", planKey: "sponsor_monthly" });
assert.equal(offerPrice(sponsor, "year").note, "Billed $199.99 yearly, save 44%");
assert.equal(bestYearlySavings(BUILTIN_PLAN_CATALOG), 44);

assert.equal(upgradePlanKey(BUILTIN_PLAN_CATALOG, "year"), "pro_yearly");
assert.equal(upgradePlanKey(BUILTIN_PLAN_CATALOG, "month"), "pro_monthly");

const [freeOffer, proOffer] = BUILTIN_PLAN_CATALOG.offers;
const actions = (subscribed: string | null, interval: "month" | "year") =>
  BUILTIN_PLAN_CATALOG.offers.map((item) => planCardAction(BUILTIN_PLAN_CATALOG, item, interval, subscribed).kind);
assert.deepEqual(actions(null, "year"), ["none", "choose", "choose"]);
assert.deepEqual(actions("pro_yearly", "year"), ["none", "none", "manage"]);
assert.deepEqual(actions("pro_yearly", "month"), ["none", "manage", "manage"]);
assert.deepEqual(actions("sponsor_monthly", "month"), ["none", "manage", "none"]);
assert.deepEqual(actions("pro", "month"), ["none", "none", "manage"]);
assert.deepEqual(planCardAction(BUILTIN_PLAN_CATALOG, proOffer, "year", null), { kind: "choose", planKey: "pro_yearly" });
assert.deepEqual(planCardAction(BUILTIN_PLAN_CATALOG, freeOffer, "year", "pro_yearly"), { kind: "none" });

assert.deepEqual(parsePlanCatalog(JSON.parse(JSON.stringify(BUILTIN_PLAN_CATALOG))), BUILTIN_PLAN_CATALOG);
assert.equal(parsePlanCatalog({ offers: [] }), null);
assert.equal(parsePlanCatalog({ offers: [{ id: "pro", name: "Pro", prices: { year: { planKey: "y", amount: -1, currency: "USD" } } }] }), null);

void (async () => {
  const loaded = await remoteCatalogSource(async () => ({ offers: [pro] })).load();
  assert.equal(loaded.offers[0].prices.year?.amount, 60);
  await assert.rejects(remoteCatalogSource(async () => ({ nope: true })).load());
})().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
