/** Task-transition truth of one accepted report, as decided by the daemon. */
export interface ReportTaskOutcome {
  persisted: boolean;
  transitionRejected: boolean;
  cause: string | null;
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
  return { persisted: false, transitionRejected: true, cause };
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
  if (outcome?.transitionRejected) return "transition rejected";
  return status?.trim() || "reported";
}

export function reportIncidentLine(outcome: ReportTaskOutcome | null): string | null {
  if (!outcome?.transitionRejected) return null;
  const cause = outcome.cause?.trim();
  return cause
    ? `Report delivered, task transition rejected — ${cause}`
    : "Report delivered, task transition rejected";
}
