import { Fragment } from "react";
import type { AutomationHandler, AutomationHandlerResult, AutomationNode, CommandCapture, CommandResultMetadata } from "./types";

export const COMMAND_FACT = {
  label: "Command", timeout: "Timeout", outcome: "Outcome", exit: "Exit code", resources: "Resources",
  stdout: "Stdout", stderr: "Stderr", started: "Started", finished: "Finished", duration: "Duration",
} as const;

export const COMMAND_VALUE = {
  notRecorded: "Not recorded", unknown: "Unknown", unavailable: "Unavailable",
  confirmed: "Settled", unconfirmed: "Settlement unconfirmed", truncated: "truncated", complete: "complete",
} as const;

const OUTCOMES: readonly string[] = ["success", "nonzero", "error", "cancelled", "deadline", "unconfirmed"] satisfies readonly CommandResultMetadata["status"][];

/** The typed command outcome when the metadata carries a recognised one; never inferred from the outer status. */
export function commandOutcome(result: AutomationHandlerResult | null | undefined): string | null {
  const status = result?.command_result?.status;
  return typeof status === "string" && OUTCOMES.includes(status) ? status : null;
}

export function commandResultLabel(handlerId: string): string {
  return `${handlerId} command result`;
}

export function isCommandResult(handler: AutomationHandler | undefined, result: AutomationHandlerResult | null | undefined): boolean {
  return handler?.type === "command" || (result?.command_result !== undefined && result.command_result !== null);
}

export function resultHandler(node: AutomationNode, bucket: string, handlerId: string): AutomationHandler | undefined {
  const matches = node.outcomes.filter((outcome) => outcome.bucket === bucket).flatMap((outcome) => outcome.handlers.filter((handler) => handler.id === handlerId));
  return matches.length === 1 ? matches[0] : undefined;
}

export function shownAsLastAccepted(node: AutomationNode, bucket: string, handler: AutomationHandler): boolean {
  const execution = handler.last_result?.execution_id;
  return !!execution && node.last_result?.bucket === bucket && node.last_result.handlers.some((entry) => entry.handler_id === handler.id && entry.execution_id === execution);
}

function count(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function instant(value: unknown): value is number {
  return count(value) && Number.isFinite(new Date(value).getTime());
}

function capture(value: CommandCapture | null | undefined): string {
  if (value === null) return COMMAND_VALUE.unavailable;
  if (!value || !count(value.total_bytes) || !count(value.retained_bytes) || typeof value.truncated !== "boolean") return COMMAND_VALUE.unknown;
  return `${value.total_bytes} B total · ${value.retained_bytes} B retained · ${value.truncated ? COMMAND_VALUE.truncated : COMMAND_VALUE.complete}`;
}

function outcomeFacts(metadata: CommandResultMetadata | null | undefined): Array<[string, string]> {
  if (!metadata) return [COMMAND_FACT.outcome, COMMAND_FACT.exit, COMMAND_FACT.resources, COMMAND_FACT.duration, COMMAND_FACT.stdout, COMMAND_FACT.stderr, COMMAND_FACT.started, COMMAND_FACT.finished].map((term): [string, string] => [term, COMMAND_VALUE.notRecorded]);
  const started = instant(metadata.started_at) ? metadata.started_at : null;
  const finished = instant(metadata.finished_at) ? metadata.finished_at : null;
  return [
    [COMMAND_FACT.outcome, OUTCOMES.includes(metadata.status) ? metadata.status : COMMAND_VALUE.unknown],
    [COMMAND_FACT.exit, Number.isSafeInteger(metadata.exit_code) ? String(metadata.exit_code) : COMMAND_VALUE.unknown],
    [COMMAND_FACT.resources, metadata.resources_confirmed === true ? COMMAND_VALUE.confirmed : metadata.resources_confirmed === false ? COMMAND_VALUE.unconfirmed : COMMAND_VALUE.unknown],
    [COMMAND_FACT.duration, started !== null && finished !== null && finished >= started ? `${finished - started} ms` : COMMAND_VALUE.notRecorded],
    [COMMAND_FACT.stdout, capture(metadata.stdout)],
    [COMMAND_FACT.stderr, capture(metadata.stderr)],
    [COMMAND_FACT.started, started === null ? COMMAND_VALUE.notRecorded : new Date(started).toLocaleString()],
    [COMMAND_FACT.finished, finished === null ? COMMAND_VALUE.notRecorded : new Date(finished).toLocaleString()],
  ];
}

export function CommandResultFacts({ handlerId, handler, result }: { handlerId: string; handler: AutomationHandler | undefined; result?: AutomationHandlerResult | null }) {
  const supplied = handler?.label;
  const label = typeof supplied === "string" && supplied.trim() ? supplied : COMMAND_VALUE.notRecorded;
  const timeoutMs = handler?.timeout_ms;
  const timeout = count(timeoutMs) && timeoutMs > 0 ? `${timeoutMs} ms` : COMMAND_VALUE.notRecorded;
  const outcome = result === undefined ? [] : outcomeFacts(result?.command_result);
  const facts: Array<[string, string]> = [[COMMAND_FACT.label, label], ...outcome.slice(0, 4), [COMMAND_FACT.timeout, timeout], ...outcome.slice(4)];
  return <dl aria-label={commandResultLabel(handlerId)} className="ct-detail__lane basis-full break-words">
    {facts.map(([term, value]) => <Fragment key={term}><dt>{term}</dt><dd className={term === COMMAND_FACT.label ? "font-medium" : undefined}>{value}</dd></Fragment>)}
  </dl>;
}
