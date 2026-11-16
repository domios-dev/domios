import type { ToolEvent } from "@codeterm/chat-core";
import { toolCommandShape } from "./commandShape";
import type { CodetermInvocation } from "./invocation";
import type { ReportOutcomeClass, ReportTaskOutcome } from "./reportOutcome";
import { resolvedReportText } from "./templates";

/** One accepted report as the sender's own transcript records it: the call and the receipt the CLI printed. */
export interface OutboundReportCall {
  reportId: string;
  targetTabId: string | null;
  task: string | null;
  status: string | null;
  sha: string | null;
  message: string;
  taskOutcome: ReportTaskOutcome | null;
}

const OUTCOME_CLASSES: ReadonlySet<string> = new Set<ReportOutcomeClass>([
  "stale_binding_conflict",
  "structurally_not_attachable",
  "delivered_unattached",
  "transition_rejected",
]);
const RECEIPT_LINE = /^ {2}([a-z_]+) +(.*)$/;
const RECEIPT_ID = /^ {2}report_id +(\S+)\s*$/gm;
const CALLS = new WeakMap<ToolEvent, readonly OutboundReportCall[]>();
const NO_CALLS: readonly OutboundReportCall[] = [];

type Receipt = Record<string, unknown>;

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() && value !== "(none)" ? value.trim() : null;
}

function record(value: unknown): Receipt | null {
  if (value && typeof value === "object" && !Array.isArray(value)) return value as Receipt;
  if (typeof value !== "string" || !value.trim().startsWith("{")) return null;
  try {
    return record(JSON.parse(value));
  } catch {
    return null;
  }
}

/** Accepted receipts in output order: the human key/value block, or the `--json` object. */
function receipts(result: string): Receipt[] {
  const json = record(result.trim());
  if (json) return text(json.report_id) ? [json] : [];
  const found: Receipt[] = [];
  let current: Receipt | null = null;
  for (const line of result.split(/\r?\n/)) {
    const field = RECEIPT_LINE.exec(line);
    if (!field) {
      current = null;
      continue;
    }
    if (!current || field[1] in current) {
      current = {};
      found.push(current);
    }
    current[field[1]] = field[2];
  }
  return found.filter((receipt) => text(receipt.report_id));
}

function outcomeClass(value: unknown): ReportOutcomeClass | null {
  const name = text(value);
  return name && OUTCOME_CLASSES.has(name) ? (name as ReportOutcomeClass) : null;
}

function taskOutcome(receipt: Receipt): ReportTaskOutcome | null {
  const report = record(receipt.task_report);
  const reportOutcome = outcomeClass(report?.report_outcome) ?? outcomeClass(receipt.report_outcome);
  if (!report) return reportOutcome ? { persisted: false, transitionRejected: false, cause: null, reportOutcome } : null;
  return {
    persisted: report.persisted === true,
    transitionRejected: report.transition_rejected === true,
    cause: text(report.cause),
    reportOutcome,
  };
}

function flag(invocation: CodetermInvocation, name: string): string | null {
  return text(invocation.flags[name]);
}

function taskOf(receipt: Receipt, invocation: CodetermInvocation): string | null {
  const report = record(receipt.task_report);
  const plan = text(report?.plan_id) ?? flag(invocation, "plan");
  const task = text(report?.task_id) ?? flag(invocation, "task");
  return plan && task ? `${plan}/${task}` : task ?? plan;
}

function reportInvocations(event: ToolEvent): Array<{ invocation: CodetermInvocation; result: string }> {
  const shape = toolCommandShape(event);
  const rows = shape.kind === "codeterm" ? [{ invocation: shape.invocation, member: event }]
    : shape.kind === "group" ? shape.rows.flatMap((row) => row.role === "codeterm" ? [{ invocation: row.invocation, member: row.event ?? event }] : [])
      : [];
  return rows
    .filter(({ invocation }) => invocation.path[0] === "agent" && invocation.path[1] === "report")
    .map(({ invocation, member }) => ({ invocation, result: member.result ?? "" }));
}

/** Reports one finished tool call delivered, each keyed by the daemon's report id. */
export function outboundReportCalls(event: ToolEvent): readonly OutboundReportCall[] {
  const cached = CALLS.get(event);
  if (cached) return cached;
  const calls: OutboundReportCall[] = [];
  if (event.status === "success") {
    const seen = new Set<string>();
    for (const { invocation, result } of reportInvocations(event)) {
      const message = resolvedReportText(result) ?? flag(invocation, "text") ?? invocation.positionals[0] ?? "";
      for (const receipt of receipts(result)) {
        const reportId = text(receipt.report_id)!;
        if (seen.has(reportId)) continue;
        seen.add(reportId);
        calls.push({
          reportId,
          targetTabId: text(receipt.target_tab_id),
          task: taskOf(receipt, invocation),
          status: flag(invocation, "status"),
          sha: flag(invocation, "sha"),
          message,
          taskOutcome: taskOutcome(receipt),
        });
      }
    }
  }
  const result = calls.length ? calls : NO_CALLS;
  CALLS.set(event, result);
  return result;
}

/** Report ids a tool's printed output names, for rows not yet reduced to tool events. */
export function outboundReportIds(output: string | undefined): string[] {
  return output ? [...output.matchAll(RECEIPT_ID)].map((match) => match[1]) : [];
}
