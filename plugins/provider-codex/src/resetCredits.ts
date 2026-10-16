import {object, text} from "./runtimeProtocol";

import type {ResetCredits, ResetCredit, ResetCreditOutcome as ResetOutcome} from "@codeterm/plugin-sdk";

function expiry(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 8_640_000_000_000
    ? value * 1000 : null;
}

export function resetCredits(raw: unknown): ResetCredits | undefined {
  const summary = object(raw);
  const count = summary.available_count;
  if (typeof count !== "number" || !Number.isSafeInteger(count) || count < 0) return undefined;
  const credits = Array.isArray(summary.credits) ? summary.credits.flatMap(rawCredit => {
    const credit = object(rawCredit);
    const id = text(credit.id);
    if (!id) return [];
    const status: ResetCredit["status"] = credit.status === "available" || credit.status === "redeeming"
      || credit.status === "redeemed" ? credit.status : "unknown";
    return [{id, status, expiresAt: expiry(credit.expires_at),
      ...(text(credit.title) ? {title: text(credit.title)} : {}),
      ...(text(credit.description) ? {description: text(credit.description)} : {})}];
  }) : null;
  const expiries = credits?.filter(credit => credit.status === "available" && credit.expiresAt !== null)
    .map(credit => credit.expiresAt as number) ?? [];
  return {availableCount: count, nextExpiry: expiries.length ? Math.min(...expiries) : null, credits};
}

export function resetOutcome(raw: unknown): ResetOutcome | undefined {
  const outcome = object(raw).outcome;
  return outcome === "reset" || outcome === "nothingToReset" || outcome === "noCredit"
    || outcome === "alreadyRedeemed" ? outcome : undefined;
}
