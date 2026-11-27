import { formatCreditHours } from "./stateModel";
import type { CloudPromoRedemption } from "./types";

export function normalizePromoCodeInput(raw: string): string {
  return raw.trim().toUpperCase();
}

export function promoRefusalMessage(code: string | null | undefined): string {
  switch (code) {
    case "invalid_code":
      return "Code not recognized. Check for typos and try again.";
    case "expired":
      return "This code has expired.";
    case "already_redeemed":
      return "You've already redeemed this code.";
    case "disabled":
    case "exhausted":
      return "This code is no longer active.";
    case "spend_cap_exceeded":
      return "This code would exceed your monthly spend limit.";
    case "rate_limited":
    case "rate-limited":
      return "Too many attempts. Try again in a moment.";
    default:
      return "Couldn't redeem this code. Try again.";
  }
}

export interface PromoRedeemOutcomeView {
  kind: "success" | "error";
  message: string;
}

export function promoRedeemOutcome(redemption: CloudPromoRedemption, submittedCode: string): PromoRedeemOutcomeView {
  if (redemption.already_redeemed) {
    return { kind: "error", message: promoRefusalMessage("already_redeemed") };
  }
  const displayCode = normalizePromoCodeInput(submittedCode);
  return {
    kind: "success",
    message: `Added ${formatCreditHours(redemption.hours)} from ${displayCode}.`,
  };
}
