export type TickFact =
  | { kind: "not_due" }
  | { kind: "unchanged_content" }
  | { kind: "unavailable" }
  | { kind: "accepted"; decidedBy: string }
  | { kind: "pending" }
  | { kind: "failed" }
  | { kind: "suppressed"; reason: string };

export interface ClassifiedTick {
  outcome: "sent" | "skipped" | "suppressed";
  spinalVerdict: "processed" | "skipped" | "suppressed";
  spinalReason: string;
}

const SPINAL_NOT_ASKED = new Set([
  "no_adapter",
  "in_flight",
  "scope_capacity",
  "recipient_unavailable",
  "plan_missing",
  "ineligible",
  "binding_missing",
  "binding_ambiguous",
  "gate_unavailable",
]);

export function classifyTick(fact: TickFact): ClassifiedTick {
  switch (fact.kind) {
    case "not_due":
      return { outcome: "skipped", spinalVerdict: "skipped", spinalReason: "not_due" };
    case "unchanged_content":
      return { outcome: "skipped", spinalVerdict: "skipped", spinalReason: "unchanged_content" };
    case "unavailable":
      return { outcome: "suppressed", spinalVerdict: "skipped", spinalReason: "agent_unavailable" };
    case "accepted":
      return fact.decidedBy === "cadence"
        ? { outcome: "sent", spinalVerdict: "skipped", spinalReason: "actionable_cadence" }
        : { outcome: "sent", spinalVerdict: "processed", spinalReason: "delivered" };
    case "pending":
      return { outcome: "skipped", spinalVerdict: "processed", spinalReason: "pending" };
    case "failed":
      return { outcome: "suppressed", spinalVerdict: "processed", spinalReason: "delivery_failed" };
    case "suppressed":
      return {
        outcome: fact.reason === "in_flight" || fact.reason === "scope_capacity" ? "skipped" : "suppressed",
        spinalVerdict: SPINAL_NOT_ASKED.has(fact.reason) ? "skipped" : "suppressed",
        spinalReason: fact.reason,
      };
  }
}

export function formatCadence(everyMs: number): string {
  if (!Number.isFinite(everyMs) || everyMs <= 0) return "unset";
  const seconds = Math.round(everyMs / 1000);
  if (seconds % 3600 === 0) return `every ${seconds / 3600}h`;
  if (seconds % 60 === 0) return `every ${seconds / 60}m`;
  return `every ${seconds}s`;
}

export function formatTickAt(atMs: number): string {
  const date = new Date(atMs);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())} ${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}:${pad(date.getUTCSeconds())}Z`;
}

export const TICK_REASON_LABELS: Record<string, string> = {
  same_snapshot: "unchanged since last",
  unchanged_content: "unchanged since last",
  low_confidence: "supporter not confident",
  no_adapter: "supporter off",
  healthy_snapshot: "Team is healthy",
  not_due: "not due yet",
  actionable_cadence: "actionable work remains",
  delivered: "delivered",
  pending: "delivery pending",
  delivery_failed: "delivery failed",
  agent_unavailable: "agent unavailable",
};

export function presentTick(tick: { atMs: number; outcome: string; spinalReason: string }) {
  return {
    time: formatTickAt(tick.atMs),
    outcome: tick.outcome,
    reason: TICK_REASON_LABELS[tick.spinalReason] ?? tick.spinalReason.replaceAll("_", " "),
  };
}

export function formatTickRow(tick: { atMs: number; outcome: string; spinalReason: string }): string {
  const row = presentTick(tick);
  return `${row.time} · ${row.outcome} · ${row.reason}`;
}

export function isSystemDelayedMessage(message: { id: string; origin?: string } | undefined): boolean {
  return message?.origin === "system" || message?.id.startsWith("system:") === true;
}
