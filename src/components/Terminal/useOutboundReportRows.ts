import { useEffect, useMemo, useRef } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import {
  hydrateOutboundAgentReportSnapshot,
  outboundReportSnapshotRequest,
  useAgentReportProjection,
  type OutboundReportSnapshotAuthority,
} from "./agentReportProjection";
import type { EnrichedChatEntry } from "./chatEnrichment";
import type { OutboundReportEntry } from "./ChatTranscriptRow";
import { outboundReportRows, type LiveOutboundReport, type OutboundReportRows } from "./outboundReportRows";
import { assertWebRequestContext, localFetch } from "../../lib/webApi";

/** Outbound report cards for the drawn rows, enriched by this client's live accepted-report events. */
export function useOutboundReportRows(tabId: string, rows: readonly EnrichedChatEntry[], transcript: readonly NormalizedChatMessage[],
  tabTitles: ReadonlyMap<string, string>, authority: OutboundReportSnapshotAuthority): OutboundReportRows {
  const projection = useAgentReportProjection(tabId);
  const snapshotRequest = useMemo(() => outboundReportSnapshotRequest(tabId, authority), [
    tabId,
    authority.peerId,
    authority.sessionId,
    authority.bindingNonce,
    authority.bootGeneration,
    authority.runtimeEpoch,
    authority.requestContext,
  ]);
  useEffect(() => {
    if (!snapshotRequest) return;
    const controller = new AbortController();
    void hydrateOutboundAgentReportSnapshot(tabId, snapshotRequest.scopeKey, async (signal) => {
      assertWebRequestContext(authority.requestContext);
      const response = await localFetch(snapshotRequest.path, {
        method: "GET",
        headers: snapshotRequest.headers,
        signal,
      });
      assertWebRequestContext(authority.requestContext);
      if (!response.ok) throw new Error(`Outbound report snapshot refused (${response.status})`);
      const snapshot: unknown = await response.json();
      assertWebRequestContext(authority.requestContext);
      return snapshot;
    }, controller.signal).catch(() => {});
    return () => controller.abort();
  }, [tabId, snapshotRequest, authority.requestContext]);
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
      ...(report.kind ? { kind: report.kind } : {}),
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
