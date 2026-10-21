import { formatAge, formatSpan, formatUntil } from "../../../lib/timeLabels";
import { QUICK_PRESETS } from "../../Terminal/delayedSend";

export type OversightMode = "curator" | "heartbeat";

/** How the Orchestrator is kept on track; the daemon owns the schedule and reports it here. */
export interface PlanOversight {
  mode: OversightMode;
  heartbeat_interval_ms: number;
  next_at: number | null;
  last_round_at: number | null;
  /** Set while the Curator is selected but the plain heartbeat runs in its place. */
  fallback_reason: string | null;
  paused: boolean;
  paused_at: number | null;
  pause_reason: string | null;
}

export type OversightTone = "accent" | "green" | "warning" | "muted";

export interface OversightLine {
  tone: OversightTone;
  text: string;
}

export const HEARTBEAT_INTERVALS = QUICK_PRESETS
  .filter(preset => /^\d+[mh]$/.test(preset.label))
  .map(preset => ({ label: preset.label, ms: preset.resolve(0) }));

export function intervalLabel(ms: number): string {
  return HEARTBEAT_INTERVALS.find(option => option.ms === ms)?.label ?? formatSpan(ms);
}

function nextIn(prefix: string, at: number | null, now: number, approximate: boolean): string {
  if (at === null) return `${prefix} not scheduled`;
  const until = formatUntil(at, now);
  return `${prefix} ${approximate && until.startsWith("in ") ? until.replace("in ", "in ~") : until}`;
}

/** The live lines under the mode: what runs now and when it acts next. */
export function oversightLines(oversight: PlanOversight, now: number): OversightLine[] {
  if (oversight.paused) return [{ tone: "muted", text: "No rounds or reminders while the plan is paused" }];
  if (oversight.mode === "curator" && oversight.fallback_reason) {
    return [
      { tone: "warning", text: `Curator unavailable, heartbeat running: ${oversight.fallback_reason}` },
      { tone: "muted", text: nextIn("Next reminder", oversight.next_at, now, false) },
    ];
  }
  if (oversight.mode === "curator") {
    const last = oversight.last_round_at === null ? "" : `, last round ${formatAge(oversight.last_round_at, now)}`;
    return [{ tone: "accent", text: `${nextIn("Next round", oversight.next_at, now, true)}${last}` }];
  }
  return [{ tone: "accent", text: nextIn("Next reminder", oversight.next_at, now, false) }];
}

/** One line for the folded header. */
export function oversightSummary(oversight: PlanOversight, now: number): string {
  if (oversight.paused) return "Paused";
  if (oversight.mode === "curator" && !oversight.fallback_reason) return `Curator, ${nextIn("next round", oversight.next_at, now, true)}`;
  return `Heartbeat every ${intervalLabel(oversight.heartbeat_interval_ms)}, ${nextIn("next", oversight.next_at, now, false)}`;
}

export function pauseHint(oversight: Pick<PlanOversight, "paused" | "paused_at" | "pause_reason">, now: number, runningHint: string): string {
  if (!oversight.paused) return runningHint;
  const since = oversight.paused_at === null ? "Paused" : `Paused ${formatAge(oversight.paused_at, now)}`;
  return `${since}${oversight.pause_reason ? `: ${oversight.pause_reason}` : ""}. No heartbeats or dispatch until resumed.`;
}
