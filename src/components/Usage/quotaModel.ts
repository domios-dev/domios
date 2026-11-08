import type { UsageSnapshot } from "../../types/usage";

/** A pill with nothing to read shows a quiet dash; the reason stays in its accessible name and popover. */
export function unavailablePill(reason: string): { text: string; label: string } {
  return { text: "—", label: `usage limits unavailable: ${reason}` };
}

export interface QuotaLimit { label: string; usedPercent: number; resetsAt: number | null }
export interface QuotaReading { title: string; limits: QuotaLimit[] }

const QUOTA_READ_INTERVAL_MS = 60_000;

function quotaLimit(value: unknown): QuotaLimit | null {
  const limit = value as { label?: unknown; used_percent?: unknown; resets_at?: unknown } | null | undefined;
  if (typeof limit?.label !== "string" || typeof limit.used_percent !== "number" || !(limit.used_percent >= 0 && limit.used_percent <= 100)) return null;
  if (limit.resets_at !== null && (typeof limit.resets_at !== "number" || !Number.isFinite(limit.resets_at))) return null;
  return { label: limit.label, usedPercent: limit.used_percent, resetsAt: limit.resets_at };
}

/** The plugin-read quota a profile's `quota` limits source stored, or null when absent or malformed. */
export function quotaReading(snapshot: UsageSnapshot | null): QuotaReading | null {
  // Stored JSON is plugin-produced, so the declared shape is checked rather than trusted.
  const quota = (snapshot?.parsed_ok ? snapshot.usage_details?.quota : null) as { title?: unknown; limits?: unknown } | null | undefined;
  if (typeof quota?.title !== "string" || !Array.isArray(quota.limits) || !quota.limits.length) return null;
  const limits = quota.limits.map(quotaLimit);
  return limits.every(Boolean) ? { title: quota.title, limits: limits as QuotaLimit[] } : null;
}

/** The pill shows the limit closest to running out and names it. */
export function quotaPill(reading: QuotaReading, failure: string | null): { text: string; label: string; percent: number } {
  const closest = reading.limits.reduce((top, limit) => limit.usedPercent > top.usedPercent ? limit : top);
  const text = `${Math.round(closest.usedPercent)}%`;
  return { text, percent: closest.usedPercent, label: `usage limits ${text} used on ${closest.label}${failure ? `, last read failed: ${failure}` : ""}` };
}

/** Reads follow the mount and the session's own activity, at most once a minute; nothing polls. */
export function quotaReadDue(lastRequestedAt: number | null, now: number): boolean {
  return lastRequestedAt === null || now - lastRequestedAt >= QUOTA_READ_INTERVAL_MS;
}
