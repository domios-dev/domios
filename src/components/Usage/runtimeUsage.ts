import type {UsageSnapshot, UsageLatestView, RuntimeUsageProjection} from "../../types/usage";
import type {RuntimeAccountCredits} from "@codeterm/plugin-sdk";

export function runtimeAccountCreditsLabel(credits:RuntimeAccountCredits|null|undefined): string|null {
  if (!credits) return null;
  if (credits.kind === "unlimited") return "Unlimited";
  return /^-?0*(\.0*)?$/.test(credits.balance) ? null : formatAccountCredits(credits.balance);
}

export function formatAccountCredits(balance: string): string {
  const match = /^(-?)(\d+)(?:\.(\d+))?$/.exec(balance);
  if (!match) return "—";
  const fraction = (match[3] ?? "") + "000";
  const cents = BigInt(match[2]) * 100n + BigInt(fraction.slice(0, 2)) + (fraction[2] >= "5" ? 1n : 0n);
  const whole = new Intl.NumberFormat("en-US").format(cents / 100n);
  const decimals = String(cents % 100n).padStart(2, "0").replace(/0+$/, "");
  return `${match[1] && cents > 0n ? "−" : ""}${whole}${decimals ? `.${decimals}` : ""}`;
}

export function latestUsageSnapshot(snapshot: UsageLatestView | null, tabId?: string, threadId?: string | null): UsageSnapshot | null {
  if (!snapshot) return null;
  const runtime = ownRuntimeUsage(snapshot, tabId, threadId);
  if (!runtime) return snapshot.runtime_usage || snapshot.parsed_ok === undefined ? null : snapshot;
  const own = runtimeLimitsSnapshot(snapshot, runtime);
  if (runtime.billing_mode !== "subscription" || !snapshot.parsed_ok || snapshot.captured_at < (runtime.rate_limits?.captured_at ?? 0)) return own;
  return {...snapshot, resets: own.resets};
}

/** The native tab's reading that speaks for the account: its freshest subscription limits. */
export function accountRuntimeUsage(snapshot: UsageLatestView | null): RuntimeUsageProjection | null {
  return (snapshot?.runtime_usage ?? [])
    .filter(row => row.billing_mode === "subscription" && row.rate_limits)
    .reduce<RuntimeUsageProjection | null>((latest, row) => !latest || (row.rate_limits?.captured_at ?? 0) > (latest.rate_limits?.captured_at ?? 0) ? row : latest, null);
}

/** A tab borrowing a native account shows that account's limits, resolved as its own tabs resolve them. */
export function accountUsageSnapshot(snapshot: UsageLatestView | null): UsageSnapshot | null {
  if (!snapshot?.runtime_usage?.length) return snapshot?.parsed_ok === undefined ? null : snapshot;
  const reading = accountRuntimeUsage(snapshot);
  if (reading) return latestUsageSnapshot(snapshot, reading.tab_id, reading.thread_id);
  return snapshot.parsed_ok ? {...snapshot, runtime_usage: undefined} : null;
}

export function ownRuntimeUsage(snapshot: UsageLatestView | null, tabId?:string, threadId?:string | null): RuntimeUsageProjection | null {
  return snapshot?.runtime_usage?.find(row => row.tab_id === tabId && row.thread_id === threadId) ?? null;
}

export function runtimeLimitsSnapshot(snapshot:UsageLatestView, reading:RuntimeUsageProjection): UsageSnapshot {
  const windows = reading.billing_mode === "subscription" ? reading.rate_limits?.value ?? [] : [];
  const primary = windows.find(window => window.window === "primary") ?? windows[0];
  const secondary = windows.find(window => window.window === "secondary") ?? windows[1];
  return {...snapshot,id:0,provider:snapshot.provider,account_id:null,usage_details:snapshot.usage_details,
    captured_at:reading.rate_limits?.captured_at ?? reading.token_usage?.captured_at ?? reading.captured_at ?? snapshot.captured_at ?? 0,
    resets:reading.resets,
    parsed_ok:true,billing_mode:reading.billing_mode,limits_reported:windows.length > 0,
    session_pct:primary?.usedPercent ?? null,session_resets_at:primary?.resetsAt ?? null,
    weekly_pct:secondary?.usedPercent ?? null,weekly_resets_at:secondary?.resetsAt ?? null,
    spend_cents:null,spend_limit_cents:null,spend_remaining_cents:null,spend_resets_at:null,
    failure_reason:null,probe_command:null};
}

export function runtimeTokenLabel(reading:RuntimeUsageProjection): string {
  const tokens = reading.token_usage?.value;
  if (!tokens) return "—";
  const total = tokens.input_tokens + tokens.output_tokens;
  const cost = tokens.cost_micros == null ? "" : ` · $${(tokens.cost_micros / 1e6).toFixed(2)}`;
  return `${total.toLocaleString("en-US")} tokens${cost}`;
}

export function runtimeTokenBreakdown(reading:RuntimeUsageProjection): string {
  const tokens = reading.token_usage?.value;
  if (!tokens) return "Session tokens not reported";
  return `Session input: ${tokens?.input_tokens ?? 0}; cached input: ${tokens?.cache_read_tokens ?? 0}; cache write: ${tokens?.cache_write_tokens ?? 0}; output: ${tokens?.output_tokens ?? 0}${tokens?.cost_micros == null ? "" : `; cost: $${tokens.cost_micros / 1e6}`}`;
}
