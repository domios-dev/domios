import type { UsageSnapshot } from "../../types/usage";
import { formatAccountCredits } from "./runtimeUsage";

export interface BalanceAmount { currency: string; amount: string }
export interface BalanceReading { available: boolean; amounts: BalanceAmount[] }

const STALE_MS = 20 * 60 * 1000;

/** The plugin-read balance a profile's `balance` limits source stored, or null when absent or malformed. */
export function balanceReading(snapshot: UsageSnapshot | null): BalanceReading | null {
  // Stored JSON is plugin-produced, so the declared shape is checked rather than trusted.
  const balance = (snapshot?.parsed_ok ? snapshot.usage_details?.balance : null) as { available?: unknown; amounts?: unknown } | null | undefined;
  if (!balance || typeof balance.available !== "boolean" || !Array.isArray(balance.amounts)) return null;
  const amounts = balance.amounts.filter((entry): entry is BalanceAmount =>
    typeof entry?.currency === "string" && typeof entry?.amount === "string");
  return amounts.length === balance.amounts.length ? { available: balance.available, amounts } : null;
}

export function balanceLabel(reading: BalanceReading): string {
  if (!reading.amounts.length) return "no balance";
  return reading.amounts.map((entry) => `${formatAccountCredits(entry.amount)} ${entry.currency}`).join(" · ");
}

/** Why the balance is unavailable or stopped moving: a failed row, the cause stamped on a kept reading, or a request that produced no row. */
export function balanceFailure(snapshot: UsageSnapshot | null, requestError: string | null): string | null {
  if (!snapshot) return requestError;
  return snapshot.parsed_ok ? snapshot.failure_reason ?? null : snapshot.failure_reason ?? "Balance unavailable";
}

/** A missing, failed or stale reading is read again when the bubble mounts or opens. */
export function balanceNeedsRead(snapshot: UsageSnapshot | null, now: number): boolean {
  return !snapshot || !snapshot.parsed_ok || now - snapshot.captured_at > STALE_MS;
}
