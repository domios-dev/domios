import { useCallback, useSyncExternalStore } from "react";
import { listen } from "../../lib/bridge";
import { normalizeEpochMs } from "./outboundReportPlacement";

export const AGENT_REPORT_EVENT = "agent-report";

export type AgentReportOutcomeClass =
  | "stale_binding_conflict"
  | "structurally_not_attachable"
  | "delivered_unattached"
  | "transition_rejected";

/** Task-transition truth of one accepted report, as decided by the daemon. */
export interface AgentReportTaskOutcome {
  persisted: boolean;
  transition_rejected: boolean;
  plan_id?: string | null;
  task_id?: string | null;
  cause?: string | null;
  report_outcome?: AgentReportOutcomeClass | null;
}

export interface AgentReportPayloadReport {
  id: string;
  from_tab_id?: string;
  from_title?: string;
  message?: string;
  timestamp?: number;
  kind?: string | null;
  plan_id?: string | null;
  task_id?: string | null;
  status?: string | null;
  branch?: string | null;
  sha?: string | null;
  report_outcome?: AgentReportOutcomeClass | null;
  sender_position?: { source_session_id: string; message_id: string } | null;
}

export interface AgentReportEventPayload {
  target_tab_id: string;
  from_tab_id: string;
  from_title?: string;
  message?: string;
  kind?: string | null;
  report: AgentReportPayloadReport;
  task_report?: AgentReportTaskOutcome | null;
  report_outcome?: AgentReportOutcomeClass | null;
  sender_position?: { source_session_id: string; message_id: string } | null;
}

export interface OutboundAgentReportSnapshotRow {
  target_tab_id: string;
  report: AgentReportPayloadReport & { from_tab_id: string };
}

export interface OutboundReportSnapshotAuthority {
  peerId: string | null;
  sessionId: string | null;
  bindingNonce: string | null;
  bootGeneration: number | null;
  runtimeEpoch: number | null;
  requestContext: number;
}

export function outboundReportSnapshotRequest(
  tabId: string,
  authority: OutboundReportSnapshotAuthority,
): { path: string; scopeKey: string; headers: Record<string, string> } | null {
  const sessionId = text(authority.sessionId);
  const bindingNonce = text(authority.bindingNonce);
  if (tabId.startsWith("mesh:") || authority.peerId != null || !sessionId || !bindingNonce) return null;
  return {
    path: `/api/tabs/${encodeURIComponent(tabId)}/reports`,
    scopeKey: JSON.stringify([
      tabId,
      authority.peerId,
      sessionId,
      bindingNonce,
      authority.bootGeneration,
      authority.runtimeEpoch,
      authority.requestContext,
    ]),
    headers: {
      Accept: "application/vnd.domios.agent-reports.outbound+json",
      "X-CodeTerm-Tab-Id": tabId,
      "X-CodeTerm-Session-Id": sessionId,
      "X-CodeTerm-Binding-Nonce": bindingNonce,
    },
  };
}

export type AgentReportSide = "outbound" | "inbound";

export interface ProjectedAgentReport {
  reportId: string;
  side: AgentReportSide;
  senderTabId: string;
  senderTitle: string;
  targetTabId: string;
  /** The other end of this report, from the projecting tab's point of view. */
  counterpartTabId: string;
  message: string;
  kind: string | null;
  task: string | null;
  status: string | null;
  branch: string | null;
  sha: string | null;
  taskOutcome: AgentReportTaskOutcome | null;
  senderPosition: { source_session_id: string; message_id: string } | null;
  /** Epoch milliseconds — normalized once at ingest, comparable to Date.parse. */
  timestamp: number | null;
}

export interface AgentReportProjection {
  outbound: ProjectedAgentReport[];
  inbound: ProjectedAgentReport[];
}

interface AgentReportRecord {
  reportId: string;
  senderTabId: string;
  senderTitle: string;
  targetTabId: string;
  message: string;
  kind: string | null;
  task: string | null;
  status: string | null;
  branch: string | null;
  sha: string | null;
  taskOutcome: AgentReportTaskOutcome | null;
  senderPosition: { source_session_id: string; message_id: string } | null;
  timestamp: number | null;
}

const EMPTY_PROJECTION: AgentReportProjection = { outbound: [], inbound: [] };
const MAX_PROJECTED_REPORTS = 240;

/** Module-owned so a Chat remount, a tab switch or a second listener cannot
 *  project the same report_id twice. */
const records = new Map<string, AgentReportRecord>();
const projections = new Map<string, AgentReportProjection>();
const listeners = new Map<string, Set<() => void>>();
let eventSubscription: Promise<() => void> | null = null;
let eventSubscriptionGeneration = 0;
let nextSnapshotGeneration = 0;
const snapshotReads = new Map<string, { generation: number; scopeKey: string }>();

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function reportOutcome(value: unknown): AgentReportOutcomeClass | null {
  return value === "stale_binding_conflict"
    || value === "structurally_not_attachable"
    || value === "delivered_unattached"
    || value === "transition_rejected"
    ? value
    : null;
}

function taskRef(report: AgentReportPayloadReport, outcome: AgentReportTaskOutcome | null): string | null {
  const planId = text(report.plan_id) ?? text(outcome?.plan_id);
  const taskId = text(report.task_id) ?? text(outcome?.task_id);
  if (planId && taskId) return `${planId}/${taskId}`;
  return taskId ?? planId;
}

function taskOutcomeOf(payload: AgentReportEventPayload): AgentReportTaskOutcome | null {
  const outcome = payload.task_report;
  const reportOutcome = reportOutcomeValue(payload, outcome);
  if (outcome == null || typeof outcome !== "object") {
    return reportOutcome == null
      ? null
      : {
          persisted: false,
          transition_rejected: false,
          plan_id: null,
          task_id: null,
          cause: null,
          report_outcome: reportOutcome,
        };
  }
  return {
    persisted: outcome.persisted === true,
    transition_rejected: outcome.transition_rejected === true,
    plan_id: text(outcome.plan_id),
    task_id: text(outcome.task_id),
    cause: text(outcome.cause),
    report_outcome: reportOutcome,
  };
}

function reportOutcomeValue(
  payload: AgentReportEventPayload,
  taskReport: AgentReportTaskOutcome | null | undefined,
): AgentReportOutcomeClass | null {
  return reportOutcome(payload.report_outcome)
    ?? reportOutcome(taskReport?.report_outcome)
    ?? reportOutcome(payload.report?.report_outcome);
}

function invalidate(tabId: string): void {
  projections.delete(tabId);
  for (const listener of listeners.get(tabId) ?? []) listener();
}

/** Records one accepted report. Returns false when this `report_id` is already
 *  projected, which is what makes the structured event and the inbound text
 *  marker converge on a single card instead of two. */
export function ingestAgentReport(payload: AgentReportEventPayload): boolean {
  const reportId = text(payload?.report?.id);
  const senderTabId = text(payload?.from_tab_id);
  const targetTabId = text(payload?.target_tab_id);
  if (!reportId || !senderTabId || !targetTabId) return false;
  if (records.has(reportId)) return false;

  const outcome = taskOutcomeOf(payload);
  records.set(reportId, {
    reportId,
    senderTabId,
    senderTitle: text(payload.from_title) ?? text(payload.report.from_title) ?? senderTabId,
    targetTabId,
    message: text(payload.message) ?? text(payload.report.message) ?? "",
    kind: text(payload.report.kind) ?? text(payload.kind),
    task: taskRef(payload.report, outcome),
    status: text(payload.report.status),
    branch: text(payload.report.branch),
    sha: text(payload.report.sha),
    taskOutcome: outcome,
    timestamp: normalizeEpochMs(payload.report.timestamp),
    senderPosition: payload.report.sender_position ?? payload.sender_position ?? null,
  });
  while (records.size > MAX_PROJECTED_REPORTS) {
    const oldest = records.keys().next().value;
    if (oldest == null) break;
    const evicted = records.get(oldest);
    records.delete(oldest);
    if (evicted) {
      invalidate(evicted.senderTabId);
      invalidate(evicted.targetTabId);
    }
  }
  invalidate(senderTabId);
  if (targetTabId !== senderTabId) invalidate(targetTabId);
  return true;
}

export function ingestOutboundAgentReportSnapshot(
  senderTabId: string,
  snapshot: unknown,
): number {
  if (!Array.isArray(snapshot)) return 0;
  let added = 0;
  for (const value of snapshot) {
    if (typeof value !== "object" || value === null || !("report" in value) || !("target_tab_id" in value)) continue;
    const row = value as OutboundAgentReportSnapshotRow;
    if (typeof row.target_tab_id !== "string" || !row.target_tab_id || row.report?.from_tab_id !== senderTabId) continue;
    if (typeof row.report.id !== "string" || !row.report.id) continue;
    const position = row.report.sender_position;
    if (position != null && (typeof position.source_session_id !== "string" || !position.source_session_id
      || typeof position.message_id !== "string" || !position.message_id)) continue;
    const existing = records.get(row.report.id);
    if (existing) {
      if (existing.senderTabId !== senderTabId || existing.targetTabId !== row.target_tab_id) continue;
      const senderPosition = position ?? null;
      const kind = text(row.report.kind);
      if ((!existing.senderPosition && senderPosition) || (!existing.kind && kind)) {
        records.set(row.report.id, {
          ...existing,
          senderPosition: existing.senderPosition ?? senderPosition,
          kind: existing.kind ?? kind,
        });
        invalidate(senderTabId);
        if (row.target_tab_id !== senderTabId) invalidate(row.target_tab_id);
      }
      continue;
    }
    if (ingestAgentReport({
      target_tab_id: row.target_tab_id,
      from_tab_id: senderTabId,
      from_title: row.report.from_title,
      message: row.report.message,
      kind: row.report.kind,
      sender_position: row.report.sender_position,
      report: row.report,
    })) added += 1;
  }
  return added;
}

function projected(record: AgentReportRecord, side: AgentReportSide): ProjectedAgentReport {
  return {
    ...record,
    side,
    counterpartTabId: side === "outbound" ? record.targetTabId : record.senderTabId,
  };
}

/** One card per report_id per side: a self-addressed report stays outbound only,
 *  so no tab can ever render the same report twice. */
export function readAgentReportProjection(tabId: string): AgentReportProjection {
  const cached = projections.get(tabId);
  if (cached) return cached;
  const outbound: ProjectedAgentReport[] = [];
  const inbound: ProjectedAgentReport[] = [];
  for (const record of records.values()) {
    if (record.senderTabId === tabId) outbound.push(projected(record, "outbound"));
    else if (record.targetTabId === tabId) inbound.push(projected(record, "inbound"));
  }
  const projection = outbound.length === 0 && inbound.length === 0
    ? EMPTY_PROJECTION
    : { outbound, inbound };
  projections.set(tabId, projection);
  return projection;
}

export function agentReportById(reportId: string): ProjectedAgentReport | null {
  const record = records.get(reportId);
  return record ? projected(record, "outbound") : null;
}

function ensureAgentReportListener(): Promise<() => void> {
  if (eventSubscription) return eventSubscription;
  const generation = ++eventSubscriptionGeneration;
  let subscription: Promise<() => void>;
  subscription = listen<AgentReportEventPayload>(AGENT_REPORT_EVENT, (event) => {
    if (generation === eventSubscriptionGeneration) ingestAgentReport(event.payload);
  }).then((unlisten) => {
    if (generation !== eventSubscriptionGeneration) {
      unlisten();
      return () => {};
    }
    return unlisten;
  }).catch((error: unknown) => {
    if (generation === eventSubscriptionGeneration) eventSubscription = null;
    throw error;
  });
  eventSubscription = subscription;
  return subscription;
}

/** The subscription the hook uses; exported so non-React owners and fixtures
 *  drive the same listener ownership instead of a parallel one. */
export function subscribeAgentReportProjection(tabId: string, onChange: () => void): () => void {
  void ensureAgentReportListener().catch(() => {});
  const existing = listeners.get(tabId) ?? new Set<() => void>();
  existing.add(onChange);
  listeners.set(tabId, existing);
  return () => {
    existing.delete(onChange);
    if (existing.size === 0) listeners.delete(tabId);
  };
}

export async function hydrateOutboundAgentReportSnapshot(
  senderTabId: string,
  scopeKey: string,
  read: (signal: AbortSignal) => Promise<unknown>,
  signal: AbortSignal,
): Promise<boolean> {
  const generation = ++nextSnapshotGeneration;
  snapshotReads.set(senderTabId, { generation, scopeKey });
  const forget = () => {
    if (snapshotReads.get(senderTabId)?.generation === generation) snapshotReads.delete(senderTabId);
  };
  signal.addEventListener("abort", forget, { once: true });
  if (signal.aborted) forget();
  const isCurrent = () => !signal.aborted
    && snapshotReads.get(senderTabId)?.generation === generation
    && snapshotReads.get(senderTabId)?.scopeKey === scopeKey;
  try {
    await ensureAgentReportListener().catch(() => {});
    if (!isCurrent()) return false;
    const snapshot = await read(signal);
    if (!isCurrent()) return false;
    ingestOutboundAgentReportSnapshot(senderTabId, snapshot);
    return true;
  } finally {
    signal.removeEventListener("abort", forget);
    forget();
  }
}

export function useAgentReportProjection(tabId: string): AgentReportProjection {
  const subscribeToTab = useCallback(
    (onChange: () => void) => subscribeAgentReportProjection(tabId, onChange),
    [tabId],
  );
  const read = useCallback(() => readAgentReportProjection(tabId), [tabId]);
  return useSyncExternalStore(subscribeToTab, read, read);
}

export function resetAgentReportProjection(): void {
  const affected = new Set<string>();
  for (const record of records.values()) {
    affected.add(record.senderTabId);
    affected.add(record.targetTabId);
  }
  records.clear();
  projections.clear();
  snapshotReads.clear();
  eventSubscriptionGeneration += 1;
  const subscription = eventSubscription;
  eventSubscription = null;
  void subscription?.then((unlisten) => unlisten()).catch(() => {});
  for (const tabId of affected) {
    for (const listener of listeners.get(tabId) ?? []) listener();
  }
}
