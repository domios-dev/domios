import type {UsageSnapshot, RuntimeUsageProjection} from "../../types/usage";

export function ownRuntimeUsage(snapshot: UsageSnapshot | null, tabId?:string, threadId?:string | null): RuntimeUsageProjection | null {
  return snapshot?.runtime_usage?.find(row => row.tab_id === tabId && row.thread_id === threadId) ?? null;
}

export function runtimeLimitsSnapshot(snapshot:UsageSnapshot, reading:RuntimeUsageProjection): UsageSnapshot {
  const windows = reading.billing_mode === "subscription" ? reading.rate_limits?.value ?? [] : [];
  const primary = windows.find(window => window.window === "primary") ?? windows[0];
  const secondary = windows.find(window => window.window === "secondary") ?? windows[1];
  return {...snapshot,id:0,provider:snapshot.provider,account_id:null,
    captured_at:reading.rate_limits?.captured_at ?? reading.token_usage?.captured_at ?? reading.captured_at ?? snapshot.captured_at,
    parsed_ok:true,billing_mode:reading.billing_mode,limits_reported:windows.length > 0,
    session_pct:primary?.usedPercent ?? null,session_resets_at:primary?.resetsAt ?? null,
    weekly_pct:secondary?.usedPercent ?? null,weekly_resets_at:secondary?.resetsAt ?? null,
    spend_cents:null,spend_limit_cents:null,spend_remaining_cents:null,spend_resets_at:null,
    failure_reason:null,probe_command:null};
}

export function runtimeTokenLabel(reading:RuntimeUsageProjection): string {
  const tokens = reading.token_usage?.value;
  if (!tokens) return "tokens not reported";
  const total = tokens.input_tokens + tokens.output_tokens;
  const cost = tokens.cost_micros == null ? "" : ` · $${(tokens.cost_micros / 1e6).toFixed(2)}`;
  return `${total.toLocaleString("en-US")} tokens${cost}`;
}

export function runtimeTokenBreakdown(reading:RuntimeUsageProjection): string {
  const tokens = reading.token_usage?.value;
  if (!tokens) return "Session tokens not reported";
  return `Session input: ${tokens?.input_tokens ?? 0}; cached input: ${tokens?.cache_read_tokens ?? 0}; cache write: ${tokens?.cache_write_tokens ?? 0}; output: ${tokens?.output_tokens ?? 0}${tokens?.cost_micros == null ? "" : `; cost: $${tokens.cost_micros / 1e6}`}`;
}
