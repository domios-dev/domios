import { describe, expect, it } from "vitest";
import { BUILTIN_PLAN_CATALOG } from "../../components/billing/planCatalog";
import { normalizePromoCodeInput, promoRedeemOutcome, promoRefusalMessage } from "./promoModel";
import {
  cloudCreateDecision,
  cloudFailurePresentation,
  cloudMachineCardView,
  cloudSizeOptions,
  formatCloudUptime,
  formatCreditHours,
} from "./stateModel";
import {
  evaluateTopUpSelection,
  nextTierUpgradeLink,
  proComparisonStrip,
  realWorkEstimate,
  resolveTopUpCatalog,
} from "./topUpModel";
import type { CloudUsagePayload } from "./types";
import { cloudUsageMeterView } from "./usageModel";

const SAMPLE_USAGE: CloudUsagePayload = {
  used: 6.2,
  allowance: 10,
  purchased: 35,
  remaining: 38.8,
  cap: 50,
  burn_per_size: { small: 1, large: 2 },
  currency: "USD",
  period_end_at: Date.UTC(2026, 11, 1) / 1000,
  custom_min_cents: 500,
  custom_max_cents: 50000,
  plan: "pro",
  purchased_expiry_days: 365,
  sizes: [
    { id: "small", cpu: 2, ram_gb: 4, hourly_rate_cents: 20, credits_per_hour: 1 },
    { id: "large", cpu: 4, ram_gb: 8, hourly_rate_cents: 40, credits_per_hour: 2 },
  ],
  packs: [
    { pack_id: "pack_5", price_cents: 500, hours: 25 },
    { pack_id: "pack_10", price_cents: 1000, hours: 50 },
    { pack_id: "pack_25", price_cents: 2500, hours: 125 },
  ],
  allowance_hours: { free: 0, pro: 10, max: 40, sponsor: 120 },
};

describe("cloudMachineCardView", () => {
  it("maps every lifecycle state to calm Variant A card and list copy without saying offline", () => {
    const preparing = cloudMachineCardView({ size: "small", state: "creating" }, SAMPLE_USAGE);
    expect(preparing.footerLabel).toBe("Preparing your machine…");
    expect(preparing.connecting).toBe(true);
    expect(preparing.showVersion).toBe(false);
    expect(preparing.edgeKind).toBe("preparing");
    expect(preparing.listStatus).toBe("preparing");

    const running = cloudMachineCardView({ size: "small", state: "awake" }, SAMPLE_USAGE);
    expect(running.footerLabel).toBe("Running");
    expect(running.footerSecondary).toBe("Small");
    expect(running.footerTone).toBe("ok");
    expect(running.edgeKind).toBe("solid");
    expect(running.listStatus).toBe("online");

    const asleep = cloudMachineCardView({ size: "small", state: "asleep" }, SAMPLE_USAGE);
    expect(asleep.footerLabel).toBe("Asleep");
    expect(asleep.footerSecondary).toBe("wakes when you send it work");
    expect(asleep.dimCard).toBe(true);
    expect(asleep.statusIcon).toBe("moon");
    expect(asleep.edgeKind).toBe("dashed");
    expect(asleep.listStatus).toBe("asleep");

    const waking = cloudMachineCardView({ size: "large", state: "waking" }, SAMPLE_USAGE);
    expect(waking.footerLabel).toBe("Waking up…");
    expect(waking.connecting).toBe(true);
    expect(waking.edgeKind).toBe("preparing");
    expect(waking.listStatus).toBe("waking up");

    const ooc = cloudMachineCardView(
      { size: "small", state: "out_of_credits", rate_per_hour: { amount: 0.2, currency: "USD", credits_per_hour: 1 } },
      { ...SAMPLE_USAGE, remaining: 0, purchased: 0 },
    );
    expect(ooc.footerLabel).toBe("Asleep");
    expect(ooc.blockedCard?.title).toBe("Out of cloud hours");
    expect(ooc.blockedCard?.body).toContain("$0.20/h");
    expect(ooc.blockedCard?.ctaLabel).toBe("Top up to wake");
    expect(ooc.listStatus).toBe("asleep, out of cloud hours");
    expect(ooc.edgeKind).toBe("dashed");

    const failed = cloudMachineCardView({ size: "small", state: "failed" }, SAMPLE_USAGE);
    expect(failed.footerLabel).toBe("Couldn't prepare the machine");
    expect(failed.footerTone).toBe("danger");
  });

  it("routes Create machine by plan and balance, and maps typed create failures to one action", () => {
    expect(cloudCreateDecision({ planId: "free", remaining: 0 })).toBe("free_paywall");
    expect(cloudCreateDecision({ planId: "free", remaining: 25 })).toBe("create");
    expect(cloudCreateDecision({ planId: "free", remaining: 0, fullAccess: true })).toBe("top_up");
    expect(cloudCreateDecision({ planId: "pro", remaining: 0 })).toBe("top_up");
    expect(cloudCreateDecision({ planId: "pro", remaining: 3.8 })).toBe("create");
    expect(cloudFailurePresentation("credits_exhausted").action).toBe("top_up");
    expect(cloudFailurePresentation("limit_reached").action).toBe("upgrade");
    expect(cloudFailurePresentation("provider_unavailable").action).toBe("retry");
  });

  it("derives sizes and rates from backend data without hardcoding commercial prices", () => {
    const withCatalog = cloudSizeOptions(SAMPLE_USAGE);
    expect(withCatalog.map((s) => `${s.id}:${s.rateLabel}`)).toEqual(["small:$0.20/h", "large:$0.40/h"]);

    const minimal = cloudSizeOptions({
      used: 0,
      allowance: 10,
      purchased: 0,
      remaining: 10,
      cap: 50,
      burn_per_size: { small: 1 },
    });
    expect(minimal).toHaveLength(1);
    expect(minimal[0].rateLabel).toBeNull();
    expect(formatCloudUptime(3720)).toBe("1h 2m");
    expect(formatCreditHours(38.8)).toBe("38.8 h");
  });
});

describe("cloudUsageMeterView", () => {
  it("computes remaining-first monthly and purchased buckets with calm low-balance and exhausted states", () => {
    const healthy = cloudUsageMeterView(SAMPLE_USAGE, { planId: "pro" });
    expect(healthy.remainingLabel).toBe("38.8 h");
    expect(healthy.contextLabel).toBe("left");
    expect(healthy.resetLabel).toBe("Resets Dec 1");
    expect(healthy.showBuckets).toBe(true);
    expect(healthy.monthlyBucketLabel).toBe("3.8 h monthly");
    expect(healthy.purchasedBucketLabel).toBe("35 h purchased");
    expect(healthy.lowBalance).toBe(false);
    expect(healthy.exhausted).toBe(false);

    const low = cloudUsageMeterView({
      ...SAMPLE_USAGE,
      used: 8.8,
      allowance: 10,
      purchased: 0,
      remaining: 1.2,
    }, { planId: "pro" });
    expect(low.remainingLabel).toBe("1.2 h");
    expect(low.contextLabel).toBe("left of 10 h this month");
    expect(low.showBuckets).toBe(false);
    expect(low.lowBalance).toBe(true);
    expect(low.exhausted).toBe(false);

    const exhausted = cloudUsageMeterView({
      ...SAMPLE_USAGE,
      used: 10,
      allowance: 10,
      purchased: 0,
      remaining: 0,
    }, { planId: "pro" });
    expect(exhausted.exhausted).toBe(true);
    expect(exhausted.lowBalance).toBe(false);

    const free = cloudUsageMeterView({
      ...SAMPLE_USAGE,
      used: 0,
      allowance: 0,
      purchased: 0,
      remaining: 0,
    }, { planId: "free" });
    expect(free.resetLabel).toBeNull();
  });
});

describe("topUpModel", () => {
  const catalog = resolveTopUpCatalog(SAMPLE_USAGE);

  it("anchors the middle preset pack, computes linear hours and real-terms duration, and formats contextual CTAs", () => {
    expect(catalog.packs.map((p) => `${p.amountLabel}:${p.hours}:${p.recommended}`)).toEqual([
      "$5:25:false",
      "$10:50:true",
      "$25:125:false",
    ]);
    expect(realWorkEstimate(25)).toBe("~6 days at 4 h/day");
    expect(realWorkEstimate(50)).toBe("~2 weeks at 4 h/day");
    expect(realWorkEstimate(75)).toBe("~3 weeks at 4 h/day");
    expect(realWorkEstimate(125)).toBe("~1 month at 4 h/day");

    const topup = evaluateTopUpSelection({
      selection: "pack_10",
      customRaw: "",
      catalog,
      currentRemaining: 3.8,
      mode: "topup",
    });
    expect(topup.valid).toBe(true);
    expect(topup.afterBalanceLabel).toBe("53.8 h");
    expect(topup.ctaLabel).toBe("Add 50 h for $10");
    expect(topup.error).toBeNull();

    const wake = evaluateTopUpSelection({
      selection: "pack_10",
      customRaw: "",
      catalog,
      currentRemaining: 0,
      mode: "wake",
      machineName: "Scratch",
    });
    expect(wake.ctaLabel).toBe("Pay $10 and wake Scratch");
    expect(wake.afterBalanceLabel).toBe("50 h");
  });

  it("validates whole-dollar custom amounts against backend bounds and hides bounds until invalid", () => {
    const empty = evaluateTopUpSelection({
      selection: "custom",
      customRaw: "",
      catalog,
      currentRemaining: 3.8,
      mode: "topup",
    });
    expect(empty.valid).toBe(false);
    expect(empty.error).toBeNull();
    expect(empty.ctaDisabled).toBe(true);
    expect(empty.ctaLabel).toBe("Add hours");

    const valid = evaluateTopUpSelection({
      selection: "custom",
      customRaw: "15",
      catalog,
      currentRemaining: 3.8,
      mode: "topup",
    });
    expect(valid.valid).toBe(true);
    expect(valid.hours).toBe(75);
    expect(valid.afterBalanceLabel).toBe("78.8 h");
    expect(valid.ctaLabel).toBe("Add 75 h for $15");
    expect(valid.checkoutPayload).toEqual({ pack_id: "custom", amount_cents: 1500 });

    const tooLow = evaluateTopUpSelection({
      selection: "custom",
      customRaw: "2",
      catalog,
      currentRemaining: 3.8,
      mode: "topup",
    });
    expect(tooLow.valid).toBe(false);
    expect(tooLow.error).toBe("Enter a whole dollar amount from $5 to $500.");
    expect(tooLow.afterBalanceLabel).toBeNull();
    expect(tooLow.ctaDisabled).toBe(true);
    expect(tooLow.ctaLabel).toBe("Add hours");
  });

  it("derives Pro comparison strip and next-tier upgrade link from PlanCatalog and backend allowance_hours", () => {
    const strip = proComparisonStrip(BUILTIN_PLAN_CATALOG, catalog.allowanceHours);
    expect(strip?.title).toBe("Or include 10 h every month with Pro");
    expect(strip?.subtitle).toContain("$6/mo billed yearly");
    expect(strip?.subtitle).toContain("5 mesh machines + 10 h cloud time");

    expect(nextTierUpgradeLink(BUILTIN_PLAN_CATALOG, "pro", catalog.allowanceHours)).toBe("Or move to Max, 40 h every month");
    expect(nextTierUpgradeLink(BUILTIN_PLAN_CATALOG, "sponsor", catalog.allowanceHours)).toBeNull();
  });
});

describe("promoModel", () => {
  it("normalizes promo input and maps every typed refusal and idempotent repeat", () => {
    expect(normalizePromoCodeInput("  launch25 ")).toBe("LAUNCH25");
    expect(promoRefusalMessage("invalid_code")).toBe("Code not recognized. Check for typos and try again.");
    expect(promoRefusalMessage("expired")).toBe("This code has expired.");
    expect(promoRefusalMessage("already_redeemed")).toBe("You've already redeemed this code.");
    expect(promoRefusalMessage("disabled")).toBe("This code is no longer active.");
    expect(promoRefusalMessage("exhausted")).toBe("This code is no longer active.");

    expect(
      promoRedeemOutcome(
        { schema_version: 1, status: "redeemed", already_redeemed: false, hours: 10, expires_at: null, remaining: 20, purchased: 10 },
        "launch25",
      ),
    ).toEqual({ kind: "success", message: "Added 10 h from LAUNCH25." });

    expect(
      promoRedeemOutcome(
        { schema_version: 1, status: "redeemed", already_redeemed: true, hours: 10, expires_at: null, remaining: 20, purchased: 10 },
        "launch25",
      ),
    ).toEqual({ kind: "error", message: "You've already redeemed this code." });
  });
});
