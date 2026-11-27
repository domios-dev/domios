import { formatCreditHours } from "./stateModel";
import type { CloudUsagePayload } from "./types";

const RESET_MONTH_DAY = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  timeZone: "UTC",
});

export interface CloudUsageMeterView {
  remainingLabel: string;
  contextLabel: string;
  resetLabel: string | null;
  monthlyRemaining: number;
  purchasedRemaining: number;
  monthlyPct: number;
  purchasedPct: number;
  showBuckets: boolean;
  monthlyBucketLabel: string;
  purchasedBucketLabel: string;
  lowBalance: boolean;
  exhausted: boolean;
  usedLabel: string;
  allowanceLabel: string;
}

function clampPct(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 0;
  return Math.min(100, Math.round(value * 10) / 10);
}

export function formatResetDate(periodEndAt: number | null | undefined): string | null {
  if (typeof periodEndAt !== "number" || !Number.isFinite(periodEndAt) || periodEndAt <= 0) {
    return null;
  }
  const ms = periodEndAt < 1e12 ? periodEndAt * 1000 : periodEndAt;
  return `Resets ${RESET_MONTH_DAY.format(new Date(ms))}`;
}

export function cloudHoursStatus(
  usage: CloudUsagePayload | null | undefined,
  outOfCredits = false,
): "Out of hours" | "Low on hours" | null {
  if (outOfCredits) return "Out of hours";
  if (!usage) return null;
  const { exhausted, lowBalance } = cloudUsageMeterView(usage);
  if (exhausted) return "Out of hours";
  return lowBalance ? "Low on hours" : null;
}

export function cloudUsageMeterView(
  usage: CloudUsagePayload,
  options?: { planId?: string | null },
): CloudUsageMeterView {
  const remaining = Math.max(0, Number.isFinite(usage.remaining) ? usage.remaining : 0);
  const purchased = Math.max(0, Number.isFinite(usage.purchased) ? usage.purchased : 0);
  const allowance = Math.max(0, Number.isFinite(usage.allowance) ? usage.allowance : 0);
  const used = Math.max(0, Number.isFinite(usage.used) ? usage.used : 0);

  const purchasedRemaining = Math.min(remaining, purchased);
  const monthlyRemaining = Math.max(0, Math.round((remaining - purchasedRemaining) * 1000) / 1000);
  const showBuckets = monthlyRemaining > 0 && purchasedRemaining > 0;

  const totalPool = Math.max(allowance + purchased, remaining, used + remaining);
  const rawMonthlyPct = totalPool > 0 ? (monthlyRemaining / totalPool) * 100 : 0;
  const rawPurchasedPct = totalPool > 0 ? (purchasedRemaining / totalPool) * 100 : 0;
  const monthlyPct = clampPct(rawMonthlyPct);
  const purchasedPct = clampPct(Math.min(100 - monthlyPct, rawPurchasedPct));

  const effectivePlan = options?.planId ?? usage.plan;
  const isFree = effectivePlan === "free" && allowance === 0;
  const resetLabel = isFree || allowance <= 0 ? null : formatResetDate(usage.period_end_at);

  const contextLabel =
    purchasedRemaining === 0 && allowance > 0
      ? `left of ${formatCreditHours(allowance)} this month`
      : "left";

  const usedRatio = totalPool > 0 ? used / totalPool : 0;
  const exhausted = remaining <= 0 && (Boolean(usage.warnings?.exhausted_at) || allowance > 0 || used > 0 || purchased > 0);
  const lowBalance = !exhausted && remaining > 0 && (usage.warnings
    ? Boolean(usage.warnings.pct80_at || usage.warnings.pct95_at)
    : remaining < 2 || usedRatio >= 0.8);

  return {
    remainingLabel: formatCreditHours(remaining),
    contextLabel,
    resetLabel,
    monthlyRemaining,
    purchasedRemaining,
    monthlyPct,
    purchasedPct,
    showBuckets,
    monthlyBucketLabel: `${formatCreditHours(monthlyRemaining)} monthly`,
    purchasedBucketLabel: `${formatCreditHours(purchasedRemaining)} purchased`,
    lowBalance,
    exhausted,
    usedLabel: formatCreditHours(used),
    allowanceLabel: formatCreditHours(allowance),
  };
}
