export type UsageDetails = import("../../gen/UsageSnapshotRow").UsageSnapshotRow["usage_details"];
export type UsageLatestResponse = import("../../gen/UsageLatestResponse").UsageLatestResponse;
type StoredUsageSnapshot = Extract<UsageLatestResponse, { id: number }>;
export type UsageSnapshot = Omit<StoredUsageSnapshot, "runtime_usage" | "raw_excerpt" | "debug_json" | "usage_details"> & Partial<Pick<StoredUsageSnapshot, "usage_details">> & {
  runtime_usage?: RuntimeUsageProjection[];
  resets?: RuntimeUsageProjection["resets"];
};
type RuntimeOnlyUsage = Extract<UsageLatestResponse, { runtime_usage: unknown[] }>;
export type UsageLatestView = UsageSnapshot | (Omit<RuntimeOnlyUsage, "runtime_usage"> & {
  runtime_usage: RuntimeUsageProjection[];
} & Partial<Record<Exclude<keyof UsageSnapshot, keyof RuntimeOnlyUsage>, undefined>>);
export type UsageSnapshotEvent = import("../../gen/UsageSnapshotEvent").UsageSnapshotEvent;

export type UsageState = "neutral" | "warn" | "danger";

export function classify(pct: number | null | undefined): UsageState {
  if (pct == null) return "neutral";
  if (pct >= 85) return "danger";
  if (pct >= 60) return "warn";
  return "neutral";
}

export type RuntimeUsageProjection = Omit<import("../../gen/RuntimeUsageProjection").RuntimeUsageProjection, "resets" | "account_credits" | "captured_at"> & Partial<Pick<import("../../gen/RuntimeUsageProjection").RuntimeUsageProjection, "resets" | "account_credits" | "captured_at">>;
