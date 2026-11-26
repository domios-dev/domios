import { formatMoney, offerPrice, type PlanCatalog } from "../../components/billing/planCatalog";
import { formatCreditHours, formatCurrencyAmount } from "./stateModel";
import type { CloudCreditPackOption, CloudUsagePayload } from "./types";

export type TopUpMode = "topup" | "wake" | "free";

export interface TopUpPackCard {
  id: string;
  amountCents: number;
  amountLabel: string;
  hours: number;
  hoursLabel: string;
  realEstimate: string;
  recommended: boolean;
}

export interface TopUpCatalog {
  packs: TopUpPackCard[];
  currency: string;
  unitRateCents: number | null;
  unitRateLabel: string | null;
  customMinCents: number | null;
  customMaxCents: number | null;
  rolloverDays: number | null;
  rolloverChip: string | null;
  rolloverSentence: string | null;
  allowanceHours: Partial<Record<string, number>>;
}

export function realWorkEstimate(hours: number): string {
  if (!Number.isFinite(hours) || hours <= 0) return "";
  const days = Math.max(1, Math.round(hours / 4));
  if (days < 10) return `~${days} ${days === 1 ? "day" : "days"} at 4 h/day`;
  if (days < 28) {
    const weeks = Math.max(2, Math.round(days / 7));
    return `~${weeks} weeks at 4 h/day`;
  }
  const months = Math.max(1, Math.round(days / 30));
  return `~${months} ${months === 1 ? "month" : "months"} at 4 h/day`;
}

export function rolloverChipLabel(expiryDays: number | null | undefined): string | null {
  if (typeof expiryDays !== "number" || !Number.isFinite(expiryDays) || expiryDays <= 0) return null;
  if (expiryDays === 365 || expiryDays === 360) return "Roll over 12 months";
  if (expiryDays % 30 === 0 && expiryDays >= 30) {
    const months = expiryDays / 30;
    return `Roll over ${months} ${months === 1 ? "month" : "months"}`;
  }
  return `Roll over ${Math.round(expiryDays)} days`;
}

export function rolloverDetailSentence(expiryDays: number | null | undefined): string | null {
  if (typeof expiryDays !== "number" || !Number.isFinite(expiryDays) || expiryDays <= 0) return null;
  if (expiryDays === 365 || expiryDays === 360) return "Unused hours roll over for 12 months.";
  if (expiryDays % 30 === 0 && expiryDays >= 30) {
    const months = expiryDays / 30;
    return `Unused hours roll over for ${months} ${months === 1 ? "month" : "months"}.`;
  }
  return `Unused hours roll over for ${Math.round(expiryDays)} days.`;
}

function packCardsFromBackend(packs: readonly CloudCreditPackOption[], currency: string): TopUpPackCard[] {
  const hasExplicitRecommended = packs.some((pack) => pack.recommended === true);
  const middleIndex = packs.length === 3 ? 1 : -1;
  return packs.map((pack, index) => ({
    id: pack.pack_id,
    amountCents: pack.price_cents,
    amountLabel: formatCurrencyAmount(pack.price_cents / 100, currency),
    hours: pack.hours,
    hoursLabel: `${Number.isInteger(pack.hours) ? String(pack.hours) : pack.hours.toFixed(1)} hours`,
    realEstimate: realWorkEstimate(pack.hours),
    recommended: hasExplicitRecommended ? pack.recommended === true : index === middleIndex,
  }));
}

export function resolveTopUpCatalog(usage: CloudUsagePayload | null | undefined): TopUpCatalog {
  const currency = (usage?.currency ?? "USD").toUpperCase();
  const packs = Array.isArray(usage?.packs) && usage.packs.length > 0
    ? packCardsFromBackend(usage.packs, currency)
    : [];

  const smallSize = usage?.sizes?.find((entry) => entry.id === "small");
  let unitRateCents: number | null = null;
  if (smallSize && smallSize.hourly_rate_cents > 0) {
    unitRateCents = smallSize.hourly_rate_cents;
  } else if (packs.length > 0 && packs[0].hours > 0) {
    unitRateCents = Math.round(packs[0].amountCents / packs[0].hours);
  }

  const unitRateLabel = unitRateCents !== null
    ? `${formatCurrencyAmount(unitRateCents / 100, currency)} / Small hour`
    : null;

  const customMinCents =
    typeof usage?.custom_min_cents === "number" && Number.isFinite(usage.custom_min_cents) && usage.custom_min_cents > 0
      ? usage.custom_min_cents
      : null;
  const customMaxCents =
    typeof usage?.custom_max_cents === "number" && Number.isFinite(usage.custom_max_cents) && usage.custom_max_cents >= 0
      ? usage.custom_max_cents
      : null;
  const rolloverDays =
    typeof usage?.purchased_expiry_days === "number" && Number.isFinite(usage.purchased_expiry_days) && usage.purchased_expiry_days > 0
      ? usage.purchased_expiry_days
      : null;

  return {
    packs,
    currency,
    unitRateCents,
    unitRateLabel,
    customMinCents,
    customMaxCents,
    rolloverDays,
    rolloverChip: rolloverChipLabel(rolloverDays),
    rolloverSentence: rolloverDetailSentence(rolloverDays),
    allowanceHours: usage?.allowance_hours ?? {},
  };
}

export interface TopUpEvaluation {
  valid: boolean;
  amountCents: number | null;
  hours: number | null;
  realEstimate: string | null;
  currentBalanceLabel: string;
  afterBalanceLabel: string | null;
  error: string | null;
  ctaLabel: string;
  ctaDisabled: boolean;
  checkoutPayload: { pack_id: "custom"; amount_cents: number } | { pack_id: string } | null;
}

export function evaluateTopUpSelection(input: {
  selection: string;
  customRaw: string;
  catalog: TopUpCatalog;
  currentRemaining: number;
  mode: TopUpMode;
  machineName?: string | null;
}): TopUpEvaluation {
  const { selection, customRaw, catalog, mode, machineName } = input;
  const currentRemaining = Math.max(0, Number.isFinite(input.currentRemaining) ? input.currentRemaining : 0);
  const currentBalanceLabel = formatCreditHours(currentRemaining);
  const wakeName = machineName?.trim() || "machine";

  const makeCta = (amountCents: number, hours: number | null): string => {
    const amount = formatCurrencyAmount(amountCents / 100, catalog.currency);
    if (mode === "wake") return `Pay ${amount} and wake ${wakeName}`;
    if (hours !== null) return `Add ${formatCreditHours(hours)} for ${amount}`;
    return `Pay ${amount}`;
  };

  if (selection !== "custom") {
    const pack = catalog.packs.find((item) => item.id === selection);
    if (!pack) {
      return {
        valid: false,
        amountCents: null,
        hours: null,
        realEstimate: null,
        currentBalanceLabel,
        afterBalanceLabel: null,
        error: null,
        ctaLabel: "Add hours",
        ctaDisabled: true,
        checkoutPayload: null,
      };
    }
    const exceedsRemainingCap = catalog.customMaxCents !== null && pack.amountCents > catalog.customMaxCents;
    if (exceedsRemainingCap) {
      const maxLabel = formatCurrencyAmount(catalog.customMaxCents! / 100, catalog.currency);
      return {
        valid: false,
        amountCents: pack.amountCents,
        hours: pack.hours,
        realEstimate: pack.realEstimate,
        currentBalanceLabel,
        afterBalanceLabel: null,
        error: catalog.customMaxCents === 0
          ? "You've reached your monthly cloud spend limit."
          : `Remaining monthly spend limit is ${maxLabel}.`,
        ctaLabel: "Add hours",
        ctaDisabled: true,
        checkoutPayload: null,
      };
    }
    return {
      valid: true,
      amountCents: pack.amountCents,
      hours: pack.hours,
      realEstimate: pack.realEstimate,
      currentBalanceLabel,
      afterBalanceLabel: formatCreditHours(currentRemaining + pack.hours),
      error: null,
      ctaLabel: makeCta(pack.amountCents, pack.hours),
      ctaDisabled: false,
      checkoutPayload: { pack_id: pack.id },
    };
  }

  const trimmed = customRaw.trim();
  const minCents = catalog.customMinCents;
  const maxCents = catalog.customMaxCents;

  if (maxCents !== null && maxCents === 0) {
    return {
      valid: false,
      amountCents: null,
      hours: null,
      realEstimate: null,
      currentBalanceLabel,
      afterBalanceLabel: null,
      error: "You've reached your monthly cloud spend limit.",
      ctaLabel: "Add hours",
      ctaDisabled: true,
      checkoutPayload: null,
    };
  }

  if (!trimmed) {
    return {
      valid: false,
      amountCents: null,
      hours: null,
      realEstimate: null,
      currentBalanceLabel,
      afterBalanceLabel: null,
      error: null,
      ctaLabel: "Add hours",
      ctaDisabled: true,
      checkoutPayload: null,
    };
  }

  const wholeDollars = /^\d+$/.test(trimmed) ? Number.parseInt(trimmed, 10) : NaN;
  const amountCents = Number.isSafeInteger(wholeDollars) ? wholeDollars * 100 : NaN;
  const outOfRange =
    !Number.isFinite(amountCents) ||
    amountCents <= 0 ||
    (minCents !== null && amountCents < minCents) ||
    (maxCents !== null && amountCents > maxCents);

  if (outOfRange) {
    const rangeError =
      minCents !== null && maxCents !== null
        ? `Enter a whole dollar amount from ${formatCurrencyAmount(minCents / 100, catalog.currency)} to ${formatCurrencyAmount(maxCents / 100, catalog.currency)}.`
        : minCents !== null
          ? `Enter a whole dollar amount of at least ${formatCurrencyAmount(minCents / 100, catalog.currency)}.`
          : "Enter a whole dollar amount.";
    return {
      valid: false,
      amountCents: null,
      hours: null,
      realEstimate: null,
      currentBalanceLabel,
      afterBalanceLabel: null,
      error: rangeError,
      ctaLabel: "Add hours",
      ctaDisabled: true,
      checkoutPayload: null,
    };
  }

  const hours = catalog.unitRateCents && catalog.unitRateCents > 0
    ? Math.round((amountCents / catalog.unitRateCents) * 10) / 10
    : null;

  return {
    valid: true,
    amountCents,
    hours,
    realEstimate: hours !== null ? realWorkEstimate(hours) : null,
    currentBalanceLabel,
    afterBalanceLabel: hours !== null ? formatCreditHours(currentRemaining + hours) : null,
    error: null,
    ctaLabel: makeCta(amountCents, hours),
    ctaDisabled: false,
    checkoutPayload: { pack_id: "custom", amount_cents: amountCents },
  };
}

export interface ProComparisonView {
  title: string;
  subtitle: string;
  actionLabel: string;
}

export function proComparisonStrip(
  planCatalog: PlanCatalog,
  allowanceHours: Partial<Record<string, number>>,
): ProComparisonView | null {
  const pro = planCatalog.offers.find((offer) => offer.id === "pro");
  if (!pro) return null;
  const proHours = allowanceHours.pro;
  const yearly = pro.prices.year ? offerPrice(pro, "year") : null;
  const monthly = pro.prices.month ? formatMoney(pro.prices.month.amount, pro.prices.month.currency) : null;
  const priceParts: string[] = [];
  if (yearly) priceParts.push(`${yearly.headline}/mo billed yearly`);
  if (monthly) priceParts.push(yearly ? `(${monthly} monthly)` : `${monthly}/mo`);
  const limitParts: string[] = [];
  const meshMax = pro.limits?.["mesh.machines"];
  if (typeof meshMax === "number") limitParts.push(`${meshMax} mesh machines`);
  if (typeof proHours === "number" && proHours > 0) limitParts.push(`${formatCreditHours(proHours)} cloud time`);
  const title = typeof proHours === "number" && proHours > 0
    ? `Or include ${formatCreditHours(proHours)} every month with ${pro.name}`
    : `Or include monthly cloud hours with ${pro.name}`;
  const subtitle = [priceParts.join(" "), limitParts.join(" + ")].filter(Boolean).join(" — ");
  return { title, subtitle, actionLabel: `View ${pro.name}` };
}

export function nextTierUpgradeLink(
  planCatalog: PlanCatalog,
  currentPlan: string | null | undefined,
  allowanceHours: Partial<Record<string, number>>,
): string | null {
  const current = currentPlan?.toLowerCase();
  if (current === "sponsor") return null;
  const targetId = current === "max" ? "sponsor" : "max";
  const offer = planCatalog.offers.find((item) => item.id === targetId);
  if (!offer) return null;
  const hours = allowanceHours[targetId];
  if (typeof hours === "number" && hours > 0) {
    return `Or move to ${offer.name}, ${formatCreditHours(hours)} every month`;
  }
  return `Or move to ${offer.name}`;
}
