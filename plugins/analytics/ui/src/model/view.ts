import type { AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import type { QueryOutcome } from "../../../src/plugin";

export type DashboardView =
  | { kind: "loading" }
  | { kind: "host_unavailable" }
  | { kind: "refused"; message: string }
  | { kind: "ledger_unavailable"; reason: string | null; snapshot: AnalyticsSnapshot }
  | { kind: "nothing_recorded"; snapshot: AnalyticsSnapshot }
  | { kind: "no_match"; snapshot: AnalyticsSnapshot }
  | { kind: "data"; snapshot: AnalyticsSnapshot; recordingOff: boolean };

/** A figure the host did not measure is never read as zero; each missing case gets its own state. */
export function dashboardView(outcome: QueryOutcome | null, filtersActive: boolean): DashboardView {
  if (!outcome) return { kind: "loading" };
  if (outcome.kind === "unavailable") return { kind: "host_unavailable" };
  if (outcome.kind === "refused") return { kind: "refused", message: outcome.message };
  const { snapshot } = outcome;
  const { recording, tokens } = snapshot;
  if (recording.state === "unavailable") return { kind: "ledger_unavailable", reason: recording.reason, snapshot };
  if (recording.state === "disabled") return { kind: "data", snapshot, recordingOff: true };
  if (recording.first_recorded_at === null && !filtersActive) return { kind: "nothing_recorded", snapshot };
  if (filtersActive && tokens.totals !== null && tokens.totals.turns === 0) return { kind: "no_match", snapshot };
  return { kind: "data", snapshot, recordingOff: false };
}
