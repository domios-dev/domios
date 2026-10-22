import type {ResetCredits, RuntimeAccountCredits} from "@codeterm/plugin-sdk";

export interface UsageDetails {
  limits: {kind: "session" | "weekly_all" | "weekly_scoped"; label: string; percent: number; resets_at: number | null; is_active: boolean; critical: boolean}[];
  dollar_credits: {remaining_dollars: number; resets_at: number | null}[];
  extra_usage: {enabled: boolean; effective_enabled?: boolean; disabled_reason?: string | null; decimal_places?: number; spend_limit_reached: boolean; used: number | null; limit: number | null; currency: string; settings_url: string} | null;
}

export interface UsageSnapshot {
  usage_details?: UsageDetails | null;
  resets?: ResetCredits | null;
  runtime_usage?: RuntimeUsageProjection[];
  billing_mode: "subscription" | "api" | "unknown";
  limits_reported: boolean;
  id: number;
  provider: string;
  account_id: string | null;
  captured_at: number;
  parsed_ok: boolean;
  session_pct: number | null;
  session_resets_at: number | null;
  weekly_pct: number | null;
  weekly_resets_at: number | null;
  /** Spend-based providers (e.g. cursor): cents used in the current billing
      cycle. Null for percentage-based providers. */
  spend_cents: number | null;
  /** Included spend allowance for the current billing cycle, in cents. */
  spend_limit_cents: number | null;
  /** Remaining included spend allowance for the current billing cycle, in cents. */
  spend_remaining_cents: number | null;
  /** Billing-cycle reset for spend-based providers. */
  spend_resets_at: number | null;
  /** Backend failure reason for parsed_ok=false rows. */
  failure_reason: string | null;
  /** Slash command used by the failed probe, for example /status or /usage. */
  probe_command: string | null;
}

export interface UsageSnapshotEvent {
  provider: string;
  account_id: string | null;
  snapshot_id: number;
}

export type UsageState = "neutral" | "warn" | "danger";

export function classify(pct: number | null | undefined): UsageState {
  if (pct == null) return "neutral";
  if (pct >= 85) return "danger";
  if (pct >= 60) return "warn";
  return "neutral";
}

export interface RuntimeUsageProjection {
  resets?: ResetCredits | null;
  account_credits?: RuntimeAccountCredits | null;
  captured_at?: number;
  tab_id: string;
  thread_id: string | null;
  billing_mode: "subscription" | "api" | "unknown";
  token_usage: {captured_at:number; value:{input_tokens:number;output_tokens:number;cache_read_tokens?:number;cache_write_tokens?:number;cost_micros?:number}} | null;
  rate_limits: {captured_at:number; value:{window:string;usedPercent:number;resetsAt?:number;windowMinutes?:number}[]} | null;
}
