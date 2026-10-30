import { formatAge, formatSpan, formatUntil } from "../../../lib/timeLabels";

export type OversightMode = "curator" | "heartbeat";

/** How the Orchestrator is kept on track; the daemon owns the schedule and reports it here. */
export interface PlanOversight {
  mode: OversightMode;
  heartbeat_interval_ms: number | null;
  next_at: number | null;
  last_round_at: number | null;
  /** Set while the Curator is selected but the plain heartbeat runs in its place. */
  fallback_reason: string | null;
  paused: boolean;
  paused_at: number | null;
  pause_reason: string | null;
  suspended_reason?: string | null;
  running_round?: number | null;
}

export type OversightTone = "accent" | "green" | "warning" | "muted";

export interface OversightLine {
  tone: OversightTone;
  text: string;
}

export const HEARTBEAT_INTERVALS = [15, 30, 45, 60, 90, 120]
  .map(minutes => ({ label: `${minutes}m`, ms: minutes * 60_000 }));

export function intervalLabel(ms: number | null): string {
  if (ms === null) return "machine default";
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
  if (oversight.suspended_reason) return [{ tone: "muted", text: suspensionLabel(oversight.suspended_reason) }];
  if (oversight.mode === "curator" && oversight.fallback_reason) {
    return [
      { tone: "warning", text: `Curator unavailable, heartbeat running: ${oversight.fallback_reason}` },
      { tone: "muted", text: nextIn("Next reminder", oversight.next_at, now, false) },
    ];
  }
  if (oversight.mode === "curator") {
    if (oversight.running_round != null) return [{ tone: "green", text: `Round ${oversight.running_round} running` }];
    const last = oversight.last_round_at === null ? "" : `, last round ${formatAge(oversight.last_round_at, now)}`;
    return [{ tone: "accent", text: `${nextIn("Next round", oversight.next_at, now, true)}${last}` }];
  }
  return [{ tone: "accent", text: nextIn("Next reminder", oversight.next_at, now, false) }];
}

/** One line for the folded header. */
export function oversightSummary(oversight: PlanOversight, now: number): string {
  if (oversight.paused) return "Paused";
  if (oversight.suspended_reason) return suspensionLabel(oversight.suspended_reason);
  if (oversight.running_round != null) return `Curator, round ${oversight.running_round} running`;
  if (oversight.mode === "curator" && !oversight.fallback_reason) return `Curator, ${nextIn("next round", oversight.next_at, now, true)}`;
  return `Heartbeat every ${intervalLabel(oversight.heartbeat_interval_ms)}, ${nextIn("next", oversight.next_at, now, false)}`;
}

export function suspensionLabel(reason: string): string {
  const labels: Record<string, string> = {
    reminders_disabled: "Plan reminders are disabled in Settings > Agents",
    paused: "No rounds or reminders while the plan is paused",
    waiting_on_human: "Waiting for your answer: no rounds or reminders",
    complete: "Plan complete: no rounds or reminders",
    loading: "Reading plan settings…",
    unavailable: "Plan settings are unavailable",
    disconnected: "Connect to Domios to read plan settings",
  };
  return Object.prototype.hasOwnProperty.call(labels, reason) ? labels[reason] : `Rounds suspended: ${reason.replace(/_/g, " ")}`;
}

export function pauseHint(oversight: Pick<PlanOversight, "paused" | "paused_at" | "pause_reason">, now: number, runningHint: string): string {
  if (!oversight.paused) return runningHint;
  const since = oversight.paused_at === null ? "Paused" : `Paused ${formatAge(oversight.paused_at, now)}`;
  return `${since}${oversight.pause_reason ? `: ${oversight.pause_reason}` : ""}. No heartbeats or dispatch until resumed.`;
}
