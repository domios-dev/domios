import type { ModelGroupUsage } from "../../lib/types";
import type { UsageSnapshot } from "../../types/usage";

/** True when a group's provider-declared usage carries a spend meter. A group
 *  with tokens but no reported spend renders nothing here. */
export function hasGroupSpend(usage: ModelGroupUsage | null | undefined): usage is ModelGroupUsage {
  return !!usage && usage.spend_cents != null;
}

/**
 * The snapshot shape the spend capsule renders, synthesized from a group's
 * provider-declared spend meter. `spend_limit_cents` and
 * `spend_remaining_cents` stay null when the provider could not read a live
 * limit — an honest absence, never a fabricated value.
 */
export function groupSpendSnapshot(
  provider: string,
  usage: ModelGroupUsage,
  capturedAtMs: number,
): UsageSnapshot {
  return {
    id: 0,
    provider,
    account_id: usage.group,
    captured_at: capturedAtMs,
    parsed_ok: true,
    session_pct: null,
    session_resets_at: null,
    weekly_pct: null,
    weekly_resets_at: null,
    spend_cents: usage.spend_cents ?? null,
    spend_limit_cents: usage.spend_limit_cents ?? null,
    spend_remaining_cents: usage.spend_remaining_cents ?? null,
    spend_resets_at: usage.spend_resets_at_ms ?? null,
    failure_reason: null,
    probe_command: null,
  };
}
