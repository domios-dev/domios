import { timestampMs, type NormalizedChatMessage } from "@codeterm/chat-core";
import { outboundReportCalls, outboundReportIds, type OutboundReportCall, type OutboundReportDescriptor } from "@codeterm/chat-ui";
import type { EnrichedChatEntry } from "./chatEnrichment";
import type { OutboundReportEntry } from "./ChatTranscriptRow";

/** An accepted-report event this client saw live, keyed by the daemon's report id. */
export interface LiveOutboundReport extends OutboundReportEntry {
  position: { source_session_id: string; message_id: string } | null;
}

export interface OutboundReportRows {
  /** Report cards owned by a rendered row, drawn right after it. */
  byRow: ReadonlyMap<number, readonly OutboundReportEntry[]>;
  /** Live receipts no loaded transcript row names yet; they keep the time-placed rail. */
  rail: readonly LiveOutboundReport[];
  /** Entry objects by report id, reused while their content is unchanged. */
  stable: Map<string, OutboundReportEntry>;
}

const NAMED_IDS = new WeakMap<NormalizedChatMessage, readonly string[]>();

function namedReportIds(message: NormalizedChatMessage): readonly string[] {
  const cached = NAMED_IDS.get(message);
  if (cached) return cached;
  const ids = [message.toolResult, message.toolDetail, message.type === "tool_result" ? message.content : undefined]
    .flatMap((output) => outboundReportIds(output));
  NAMED_IDS.set(message, ids);
  return ids;
}

function callDescriptor(call: OutboundReportCall, recipient: string): OutboundReportDescriptor {
  return { reportId: call.reportId, recipient, task: call.task, status: call.status, sha: call.sha, message: call.message,
    taskOutcome: call.taskOutcome };
}

function sameOutcome(left: OutboundReportDescriptor["taskOutcome"], right: OutboundReportDescriptor["taskOutcome"]): boolean {
  if (left == null || right == null) return left === right;
  return left.persisted === right.persisted && left.transitionRejected === right.transitionRejected
    && left.cause === right.cause && (left.reportOutcome ?? null) === (right.reportOutcome ?? null);
}

function sameEntry(left: OutboundReportEntry, right: OutboundReportEntry): boolean {
  const a = left.descriptor;
  const b = right.descriptor;
  return left.targetTabId === right.targetTabId && left.at === right.at && a.reportId === b.reportId && a.recipient === b.recipient
    && a.task === b.task && a.status === b.status && a.sha === b.sha && a.message === b.message && sameOutcome(a.taskOutcome, b.taskOutcome);
}

/** Outbound report cards as a projection of the sender's own transcript: each card follows the
 *  tool call that sent it, survives reload without the live store, and a live receipt only
 *  enriches the row whose printed report id it carries, so it can never render twice. */
export function outboundReportRows(args: {
  rows: readonly EnrichedChatEntry[];
  transcript: readonly NormalizedChatMessage[];
  live: readonly LiveOutboundReport[];
  recipient: (targetTabId: string | null) => string;
  stable?: ReadonlyMap<string, OutboundReportEntry>;
}): OutboundReportRows {
  const liveById = new Map(args.live.map((report) => [report.descriptor.reportId, report]));
  const stable = new Map<string, OutboundReportEntry>();
  const byRow = new Map<number, OutboundReportEntry[]>();
  const keep = (next: OutboundReportEntry) => {
    const previous = args.stable?.get(next.descriptor.reportId);
    const entry = previous && sameEntry(previous, next) ? previous : next;
    stable.set(next.descriptor.reportId, entry);
    return entry;
  };
  args.rows.forEach(({ item }, index) => {
    if (item.kind !== "tool") return;
    for (const call of outboundReportCalls(item.event)) {
      if (stable.has(call.reportId)) continue;
      const live = liveById.get(call.reportId);
      const entry = keep(live
        ? { targetTabId: live.targetTabId, at: live.at, descriptor: live.descriptor }
        : { targetTabId: call.targetTabId ?? "", at: timestampMs(item.event.message.timestamp),
          descriptor: callDescriptor(call, args.recipient(call.targetTabId)) });
      const bucket = byRow.get(index);
      if (bucket) bucket.push(entry);
      else byRow.set(index, [entry]);
    }
  });
  const named = new Set(args.transcript.flatMap(namedReportIds));
  const rail = args.live.filter((report) => !named.has(report.descriptor.reportId) && !stable.has(report.descriptor.reportId));
  return { byRow, rail, stable };
}
