import { localFetch } from "../../lib/webApi";

export type GiftDuration = "1m" | "3m" | "1y" | "3y" | "lifetime";
export interface GiftReceipt {
  schema_version: 1;
  status: "redeemed";
  plan: "pro" | "max" | "sponsor";
  duration: GiftDuration;
  expires_at: number | null;
}

export const giftDurationLabel: Record<GiftDuration, string> = {
  "1m": "1 month", "3m": "3 months", "1y": "1 year", "3y": "3 years", lifetime: "Lifetime",
};

export function parseGiftReceipt(value: unknown): GiftReceipt {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("invalid_response");
  const receipt = value as Record<string, unknown>;
  if (Object.keys(receipt).length !== 5 || receipt.schema_version !== 1 || receipt.status !== "redeemed"
    || typeof receipt.plan !== "string" || !["pro", "max", "sponsor"].includes(receipt.plan)
    || typeof receipt.duration !== "string" || !Object.prototype.hasOwnProperty.call(giftDurationLabel, receipt.duration)
    || (receipt.duration === "lifetime" ? receipt.expires_at !== null
      : typeof receipt.expires_at !== "number" || !Number.isSafeInteger(receipt.expires_at)
        || receipt.expires_at <= 0 || receipt.expires_at > 8_640_000_000_000)) throw new Error("invalid_response");
  return receipt as unknown as GiftReceipt;
}

export function giftFailure(reason: string): string {
  switch (reason) {
    case "invalid_key": return "This key is invalid, already used by another account, or revoked.";
    case "already_lifetime": return "Your account already has lifetime access for this plan. The key was not used.";
    case "rate_limited": return "Too many attempts. Try again later.";
    case "account_changed": return "Your account changed. Sign in to the intended account before trying again.";
    case "sign_in_required": return "Sign in before redeeming a gift key.";
    case "refresh_unavailable": return "Your key was redeemed, but verified access could not be refreshed. Refresh your plan to check access.";
    default: return "Redemption could not be confirmed. Refresh your plan, or enter the same key again on this account to check it.";
  }
}

export async function redeemGift(key: string, identity: string, signal: AbortSignal): Promise<GiftReceipt> {
  const response = await localFetch("/api/account/gift/redeem", { method: "POST", signal,
    headers: { "Content-Type": "application/json" }, body: JSON.stringify({ key, expected_uid: identity }) });
  if (!response.ok) {
    const reason = await response.text();
    throw new Error(["invalid_key", "already_lifetime", "rate_limited", "account_changed", "sign_in_required", "refresh_unavailable"].includes(reason) ? reason : "unavailable");
  }
  return parseGiftReceipt(await response.json());
}
