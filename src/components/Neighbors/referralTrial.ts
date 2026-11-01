export interface ReferralTrial {
  state: "granted" | "already_paid" | "none";
  trial_end: number | null;
  inviter_uid: string | null;
  friend_request_id: string | null;
}

export function parseReferralTrial(value: unknown): ReferralTrial {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Invalid referral status");
  const row = value as Record<string, unknown>;
  if (!["granted", "already_paid", "none"].includes(String(row.state))
    || (row.state === "granted" ? typeof row.trial_end !== "number" || !Number.isSafeInteger(row.trial_end) || row.trial_end <= 0 : row.trial_end !== null)
    || (row.inviter_uid !== null && (typeof row.inviter_uid !== "string" || !row.inviter_uid || row.inviter_uid.length > 512))
    || (row.friend_request_id !== null && (typeof row.friend_request_id !== "string" || !/^[a-f0-9]{64}$/.test(row.friend_request_id)))) {
    throw new Error("Invalid referral status");
  }
  return value as ReferralTrial;
}

export function referralStatus(trial: ReferralTrial, access: "paid" | "locked", formatDate: (seconds: number) => string): string | null {
  if (trial.state === "already_paid") return "Your existing plan is unchanged. You can accept the Neighbor request.";
  if (trial.state !== "granted") return null;
  return access === "locked"
    ? "Your referral trial is not providing writing access. Subscribe to write with Neighbors."
    : `Domios issued your seven-day referral trial ending ${formatDate(trial.trial_end!)}. No payment method was required. A qualifying subscription during this window earns your inviter one free month.`;
}
