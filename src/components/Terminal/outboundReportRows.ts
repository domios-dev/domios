import { timestampMs, type NormalizedChatMessage } from "@codeterm/chat-core";
import { outboundReportCalls, outboundReportIds, type OutboundReportCall, type OutboundReportDescriptor } from "@codeterm/chat-ui";
import type { EnrichedChatEntry } from "./chatEnrichment";
import type { OutboundReportEntry } from "./ChatTranscriptRow";

/** An accepted-report event this client saw live, keyed by the daemon's report id. */
export interface LiveOutboundReport extends OutboundReportEntry {
  position: { source_session_id: string; message_id: string } | null;
}

export interface OutboundReportRows {
  /** Untimed report cards anchored to their source row. */
  byRow: ReadonlyMap<number, readonly OutboundReportEntry[]>;
  /** Timestamped report cards and live receipts, placed by their authoritative time. */
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
  const leftPosition = left.position ?? null;
  const rightPosition = right.position ?? null;
  return left.targetTabId === right.targetTabId && left.at === right.at
    && leftPosition?.source_session_id === rightPosition?.source_session_id && leftPosition?.message_id === rightPosition?.message_id
    && a.reportId === b.reportId && a.recipient === b.recipient
    && (a.kind ?? null) === (b.kind ?? null)
    && a.task === b.task && a.status === b.status && a.sha === b.sha && a.message === b.message && sameOutcome(a.taskOutcome, b.taskOutcome);
}

/** Outbound report cards from transcript receipts and live host events. The live receipt
 *  enriches a matching printed report id, so the same card never renders twice. */
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
  const rail: LiveOutboundReport[] = [];
  const namedTimes = new Map<string, number>();
  for (const message of args.transcript) {
    const at = timestampMs(message.timestamp);
    if (at == null) continue;
    for (const reportId of namedReportIds(message)) namedTimes.set(reportId, at);
  }
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
      const source = item.event.message;
      const position = live?.position ?? {
        source_session_id: source.sourceSessionId ?? "",
        message_id: source.sourceMessageId ?? source.id,
      };
      const entry = keep(live
        ? { targetTabId: live.targetTabId, at: live.at ?? namedTimes.get(call.reportId) ?? timestampMs(source.timestamp), descriptor: live.descriptor, position }
        : { targetTabId: call.targetTabId ?? "", at: namedTimes.get(call.reportId) ?? timestampMs(source.timestamp), position,
          descriptor: callDescriptor(call, args.recipient(call.targetTabId)) });
      if (entry.at != null) rail.push({ ...entry, position });
      else {
        const bucket = byRow.get(index);
        if (bucket) bucket.push(entry);
        else byRow.set(index, [entry]);
      }
    }
  });
  for (const report of args.live) {
    if (!stable.has(report.descriptor.reportId)) rail.push({ ...keep(report), position: report.position });
  }
  rail.sort((left, right) => {
    if (left.at != null && right.at != null && left.at !== right.at) return left.at - right.at;
    if (left.at != null && right.at == null) return -1;
    if (left.at == null && right.at != null) return 1;
    return left.descriptor.reportId < right.descriptor.reportId ? -1 : left.descriptor.reportId > right.descriptor.reportId ? 1 : 0;
  });
  return { byRow, rail, stable };
}
