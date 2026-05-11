export interface UsageSnapshot {
  id: number;
  provider: string;
  account_id: string | null;
  captured_at: number;
  parsed_ok: boolean;
  session_pct: number | null;
  session_resets_at: number | null;
  weekly_pct: number | null;
  weekly_resets_at: number | null;
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
