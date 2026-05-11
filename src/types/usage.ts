export type BreakdownDimension = "skill" | "subagent" | "plugin" | "heuristic";

export interface UsageBreakdownRow {
  dimension: BreakdownDimension;
  key: string;
  pct: number;
}

export interface UsageSnapshot {
  id: number;
  provider: string;
  account_id: string | null;
  captured_at: number;
  parsed_ok: boolean;
  parse_error: string | null;
  raw_text: string | null;
  session_pct: number | null;
  session_resets_at: number | null;
  weekly_pct: number | null;
  weekly_resets_at: number | null;
  weekly_sonnet_pct: number | null;
  weekly_sonnet_resets_at: number | null;
  session_cost_usd: number | null;
  session_api_duration_s: number | null;
  session_wall_duration_s: number | null;
  session_lines_added: number | null;
  session_lines_removed: number | null;
  session_tokens_input: number | null;
  session_tokens_output: number | null;
  session_tokens_cache_read: number | null;
  session_tokens_cache_write: number | null;
  breakdown: UsageBreakdownRow[];
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
