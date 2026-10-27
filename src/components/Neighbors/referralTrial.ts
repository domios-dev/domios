export type ReferralOutcome = "granted" | "already_granted" | "not_invited" | "inviter_not_paid" | "already_paid" | "not_fresh" | "invite_expired";

export interface ReferralClaim {
  schema_version: 1;
  outcome: ReferralOutcome;
  trial_end: number | null;
  friend_request_id: string | null;
}

const outcomes: readonly string[] = ["granted", "already_granted", "not_invited", "inviter_not_paid", "already_paid", "not_fresh", "invite_expired"];

export function parseReferralClaim(value: unknown): ReferralClaim {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Invalid referral response");
  const row = value as Record<string, unknown>;
  const granted = row.outcome === "granted" || row.outcome === "already_granted";
  if (Object.keys(row).some(key => !["schema_version", "outcome", "trial_end", "friend_request_id"].includes(key))
    || row.schema_version !== 1 || !outcomes.includes(String(row.outcome))
    || (granted ? typeof row.trial_end !== "number" || !Number.isSafeInteger(row.trial_end) || row.trial_end <= 0 : row.trial_end !== null)
    || (row.friend_request_id !== null && (typeof row.friend_request_id !== "string" || !/^[a-f0-9]{64}$/.test(row.friend_request_id)))) {
    throw new Error("Invalid referral response");
  }
  return value as ReferralClaim;
}

export function referralStatus(claim: ReferralClaim, access: "paid" | "locked", formatDate: (seconds: number) => string): string | null {
  switch (claim.outcome) {
    case "granted":
    case "already_granted":
      return access === "locked"
        ? "Your referral trial is not providing writing access. Subscribe to write with Neighbors."
        : `Domios issued your seven-day referral trial ending ${formatDate(claim.trial_end!)}. No payment method was required. A qualifying subscription during this window earns your inviter one free month.`;
    case "already_paid": return "Your existing plan is unchanged. You can accept the Neighbor request.";
    case "inviter_not_paid": return "This invitation cannot start a referral trial because the inviter has no qualifying paid subscription.";
    case "not_fresh": return "This account is not eligible for a new referral trial. You can still accept Neighbor requests.";
    case "invite_expired": return "This referral invitation has expired. Ask your inviter for help.";
    case "not_invited": return null;
  }
}
