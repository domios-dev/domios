export type ReportOutcomeClass =
  | "stale_binding_conflict"
  | "structurally_not_attachable"
  | "delivered_unattached";

/** Task-transition truth of one accepted report, as decided by the daemon. */
export interface ReportTaskOutcome {
  persisted: boolean;
  transitionRejected: boolean;
  cause: string | null;
  reportOutcome?: ReportOutcomeClass | null;
}

const REJECTED_HEADLINE = "TASK TRANSITION REJECTED";
const CAUSE_LINE = /^Owner action required:\s*(.+)$/;

/** A refused transition is delivered with its own incident header, so a card
 *  with no structured event linked can still be truthful. Only a body that
 *  opens with the header counts; quoted prose further down never does. */
export function parseDeliveredTaskOutcome(body: string): ReportTaskOutcome | null {
  const lines = body.split("\n");
  if (!lines[0]?.trim().endsWith(REJECTED_HEADLINE)) return null;
  let cause: string | null = null;
  for (const line of lines.slice(1, 4)) {
    const match = CAUSE_LINE.exec(line.trim());
    if (match) {
      cause = match[1].trim();
      break;
    }
  }
  return {
    persisted: false,
    transitionRejected: true,
    cause,
    reportOutcome: "stale_binding_conflict",
  };
}

/** The report's own content, with the delivered incident header lifted out so
 *  the card shows the summary first instead of repeating the header. */
export function reportContentBody(body: string): string {
  if (!parseDeliveredTaskOutcome(body)) return body;
  const lines = body.split("\n");
  let index = 1;
  while (index < lines.length && lines[index].trim() !== "") index += 1;
  return lines.slice(index).join("\n").trim();
}

export function shortSha(sha: string | null | undefined): string | null {
  const trimmed = sha?.trim();
  if (!trimmed) return null;
  return trimmed.length > 9 ? trimmed.slice(0, 9) : trimmed;
}

export function reportVerdict(
  status: string | null | undefined,
  outcome: ReportTaskOutcome | null,
): string {
  if (outcome?.reportOutcome === "stale_binding_conflict" || outcome?.transitionRejected) {
    return "transition rejected";
  }
  if (outcome?.reportOutcome === "structurally_not_attachable") return "reported";
  if (outcome?.reportOutcome === "delivered_unattached") return "delivered";
  return status?.trim() || "reported";
}

export function reportOutcomeLine(outcome: ReportTaskOutcome | null): string | null {
  if (outcome?.reportOutcome === "stale_binding_conflict" || outcome?.transitionRejected) {
    const cause = outcome.cause?.trim();
    return cause
      ? `Report delivered, task transition rejected — ${cause}`
      : "Report delivered, task transition rejected";
  }
  if (outcome?.reportOutcome === "structurally_not_attachable") return null;
  if (outcome?.reportOutcome === "delivered_unattached") return "Report delivered, not attached to a task";
  return null;
}

/** Compatibility name for callers that still render the legacy refusal row. */
export const reportIncidentLine = reportOutcomeLine;

export function reportOutcomeTone(outcome: ReportTaskOutcome | null): "error" | "calm" | null {
  if (outcome?.reportOutcome === "stale_binding_conflict" || outcome?.transitionRejected) return "error";
  if (
    outcome?.reportOutcome === "structurally_not_attachable"
    || outcome?.reportOutcome === "delivered_unattached"
  ) return "calm";
  return null;
}
