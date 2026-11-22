import {usageField, usageRecord} from "./usageDetailsModel";
import type { UsageLatestView } from "../../types/usage";
import { modelGroupForId } from "../../lib/modelGroups";
import type { ModelGroup } from "../../lib/types";
import type { VisibleUsageMetric } from "./metrics";

/** A pill with nothing to read shows a quiet dash; the reason stays in its accessible name and popover. */
export function unavailablePill(reason: string): { text: string; label: string } {
  return { text: "—", label: `usage limits unavailable: ${reason}` };
}

export interface QuotaLimit { label: string; usedPercent: number; resetsAt: number | null }
export interface QuotaReading { title: string; limits: QuotaLimit[] }
export interface QuotaResetReading { resetsAt: number; observedAt: number | null; reason: string | null }
export interface QuotaGroup { label: string; limits: QuotaLimit[]; metrics: Pick<VisibleUsageMetric, "key" | "pct">[] }

const QUOTA_READ_INTERVAL_MS = 60_000;
const MINUTE_MS = 60_000;
const HOUR_MS = 3_600_000;

function quotaLimit(value: unknown): QuotaLimit | null {
  const limit = value;
  if (!usageRecord(limit) || typeof limit.label !== "string" || typeof limit.used_percent !== "number" || !(limit.used_percent >= 0 && limit.used_percent <= 100)) return null;
  if (limit.resets_at !== null && (typeof limit.resets_at !== "number" || !Number.isFinite(limit.resets_at))) return null;
  return { label: limit.label, usedPercent: limit.used_percent, resetsAt: limit.resets_at };
}

/** The plugin-read quota a profile's `quota` limits source stored, or null when absent or malformed. */
export function quotaReading(snapshot: UsageLatestView | null): QuotaReading | null {
  // Stored JSON is plugin-produced, so the declared shape is checked rather than trusted.
  const quota = snapshot?.parsed_ok ? usageField(snapshot.usage_details, "quota") : null;
  if (!usageRecord(quota) || typeof quota.title !== "string" || !Array.isArray(quota.limits) || !quota.limits.length) return null;
  const limits = quota.limits.map(quotaLimit);
  return limits.every((limit): limit is QuotaLimit => limit !== null) ? { title: quota.title, limits } : null;
}

/** The pill shows the limit closest to running out and names it. */
export function quotaPill(reading: QuotaReading, failure: string | null): { text: string; label: string; percent: number } {
  const closest = reading.limits.reduce((top, limit) => limit.usedPercent > top.usedPercent ? limit : top);
  const text = `${Math.round(closest.usedPercent)}%`;
  return { text, percent: closest.usedPercent, label: `usage limits ${text} used on ${closest.label}${failure ? `, last read failed: ${failure}` : ""}` };
}

export function quotaGroups(reading: QuotaReading): QuotaGroup[] {
  const groups = new Map<string, QuotaGroup>();
  for (const limit of reading.limits) {
    const bucket = /^(.+) (5h|weekly)$/i.exec(limit.label);
    if (!bucket) continue;
    const [, label, window] = bucket;
    const id = label.toLowerCase();
    const group = groups.get(id) ?? { label, limits: [], metrics: [] };
    group.limits.push(limit);
    const key = window.toLowerCase() === "5h" ? "session" : "weekly";
    if (!group.metrics.some(metric => metric.key === key)) group.metrics.push({ key, pct: limit.usedPercent });
    groups.set(id, group);
  }
  return [...groups.values()].map(group => ({
    ...group,
    metrics: [...group.metrics].sort((a, b) => a.key.localeCompare(b.key)),
  }));
}

export function activeQuotaGroup(groups: QuotaGroup[], modelId?: string | null, modelGroups?: ModelGroup[]): QuotaGroup | null {
  if (!modelId) return groups.length === 1 ? groups[0] : null;
  const declared = modelGroupForId(modelGroups, modelId);
  const model = modelId.slice(modelId.lastIndexOf("/") + 1).toLowerCase();
  const key = (label: string) => label.toLowerCase().replace(/\s+/g, "-");
  if (declared) {
    const group = groups.find(group => key(group.label) === key(declared.id) || key(group.label) === key(declared.label));
    if (group) return group;
  }
  const matches = groups.filter(group => model === key(group.label) || model.startsWith(`${key(group.label)}-`));
  return matches.length === 1 ? matches[0] : null;
}

/** Reads follow the mount and the session's own activity, at most once a minute; nothing polls. */
export function quotaReadDue(lastRequestedAt: number | null, now: number): boolean {
  return lastRequestedAt === null || now - lastRequestedAt >= QUOTA_READ_INTERVAL_MS;
}

/** An observed profile rate-limit reset that has not elapsed yet, or null once reset or absent. */
export function activeQuotaReset(snapshot: UsageLatestView | null, now: number): QuotaResetReading | null {
  const raw = snapshot ? usageField(snapshot.usage_details, "quota_reset") : null;
  if (!usageRecord(raw) || typeof raw.resets_at !== "number" || !Number.isFinite(raw.resets_at) || raw.resets_at <= now) {
    return null;
  }
  const observedAt = typeof raw.observed_at === "number" && Number.isFinite(raw.observed_at) ? raw.observed_at : null;
  const reason = typeof raw.reason === "string" && raw.reason.trim().length > 0 ? raw.reason.trim() : null;
  return { resetsAt: raw.resets_at, observedAt, reason };
}

export function formatResetCountdown(resetsAt: number, now: number): string {
  const remainMs = Math.max(0, resetsAt - now);
  if (remainMs < MINUTE_MS) return "<1m";
  const hours = Math.floor(remainMs / HOUR_MS);
  const minutes = Math.floor((remainMs % HOUR_MS) / MINUTE_MS);
  if (hours > 0 && minutes > 0) return `${hours}h ${minutes}m`;
  if (hours > 0) return `${hours}h`;
  return `${minutes}m`;
}

export function quotaResetPill(reset: QuotaResetReading, now: number): { text: string; label: string } {
  const countdown = formatResetCountdown(reset.resetsAt, now);
  return { text: `Quota · resets in ${countdown}`, label: `quota limit reached, resets in ${countdown}` };
}

export function nextQuotaResetTickMs(resetsAt: number, now: number): number | null {
  const remain = resetsAt - now;
  if (!(remain > 0)) return null;
  if (remain <= MINUTE_MS) return remain + 50;
  return (remain % MINUTE_MS) || MINUTE_MS;
}
