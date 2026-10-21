import type {ResetCredits} from "@codeterm/plugin-sdk";
import type {UsageDetails} from "../../types/usage";
import {availableResetCredits} from "./resetCreditsModel";

const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;

export function creditExpiryHint(expiresAt: number | null | undefined, now: number): string | null {
  if (expiresAt == null || !Number.isFinite(expiresAt)) return null;
  const remaining = expiresAt - now;
  if (remaining <= 0 || remaining > 3 * DAY_MS) return null;
  const count = Math.ceil(remaining / (remaining < DAY_MS ? HOUR_MS : DAY_MS));
  const unit = remaining < DAY_MS ? "hour" : "day";
  return `expires in ${count} ${unit}${count === 1 ? "" : "s"}`;
}

export function hasExpiringCredits(resets: ResetCredits | null | undefined, details: UsageDetails | null | undefined, now: number): boolean {
  const resetAlert = !!resets && resets.availableCount > 0 && (
    availableResetCredits(resets).some(credit => creditExpiryHint(credit.expiresAt, now) !== null)
    || ((resets.credits === null || resets.credits.length < resets.availableCount) && creditExpiryHint(resets.nextExpiry, now) !== null));
  return resetAlert || !!details?.dollar_credits.some(credit => credit.remaining_dollars > 0 && creditExpiryHint(credit.resets_at, now) !== null);
}
