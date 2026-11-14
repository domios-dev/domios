import { useMemo, useRef } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { useAgentReportProjection } from "./agentReportProjection";
import type { EnrichedChatEntry } from "./chatEnrichment";
import type { OutboundReportEntry } from "./ChatTranscriptRow";
import { outboundReportRows, type LiveOutboundReport, type OutboundReportRows } from "./outboundReportRows";

/** Outbound report cards for the drawn rows, enriched by this client's live accepted-report events. */
export function useOutboundReportRows(tabId: string, rows: readonly EnrichedChatEntry[], transcript: readonly NormalizedChatMessage[],
  tabTitles: ReadonlyMap<string, string>): OutboundReportRows {
  const projection = useAgentReportProjection(tabId);
  const recipient = useMemo(() => (targetTabId: string | null) => {
    if (!targetTabId) return "parent";
    const short = targetTabId.slice(0, 8);
    return tabTitles.get(short) ?? short;
  }, [tabTitles]);
  const live = useMemo<LiveOutboundReport[]>(() => projection.outbound.map((report) => ({
    targetTabId: report.targetTabId,
    at: report.timestamp,
    position: report.senderPosition,
    descriptor: {
      reportId: report.reportId,
      recipient: recipient(report.targetTabId),
      task: report.task,
      status: report.status,
      sha: report.sha,
      message: report.message,
      taskOutcome: report.taskOutcome && {
        persisted: report.taskOutcome.persisted,
        transitionRejected: report.taskOutcome.transition_rejected,
        cause: report.taskOutcome.cause ?? null,
        reportOutcome: report.taskOutcome.report_outcome ?? null,
      },
    },
  })), [projection, recipient]);
  const stable = useRef<Map<string, OutboundReportEntry>>(new Map());
  return useMemo(() => {
    const placed = outboundReportRows({ rows, transcript, live, recipient, stable: stable.current });
    stable.current = placed.stable;
    return placed;
  }, [rows, transcript, live, recipient]);
}
