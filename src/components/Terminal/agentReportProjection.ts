import { useCallback, useSyncExternalStore } from "react";
import { listen } from "../../lib/bridge";

export const AGENT_REPORT_EVENT = "agent-report";

/** Task-transition truth of one accepted report, as decided by the daemon. */
export interface AgentReportTaskOutcome {
  persisted: boolean;
  transition_rejected: boolean;
  plan_id?: string | null;
  task_id?: string | null;
  cause?: string | null;
}

export interface AgentReportPayloadReport {
  id: string;
  from_tab_id?: string;
  from_title?: string;
  message?: string;
  timestamp?: number;
  plan_id?: string | null;
  task_id?: string | null;
  status?: string | null;
  branch?: string | null;
  sha?: string | null;
}

export interface AgentReportEventPayload {
  target_tab_id: string;
  from_tab_id: string;
  from_title?: string;
  message?: string;
  report: AgentReportPayloadReport;
  task_report?: AgentReportTaskOutcome | null;
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
  task: string | null;
  status: string | null;
  branch: string | null;
  sha: string | null;
  taskOutcome: AgentReportTaskOutcome | null;
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
  task: string | null;
  status: string | null;
  branch: string | null;
  sha: string | null;
  taskOutcome: AgentReportTaskOutcome | null;
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

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function taskRef(report: AgentReportPayloadReport, outcome: AgentReportTaskOutcome | null): string | null {
  const planId = text(report.plan_id) ?? text(outcome?.plan_id);
  const taskId = text(report.task_id) ?? text(outcome?.task_id);
  if (planId && taskId) return `${planId}/${taskId}`;
  return taskId ?? planId;
}

function taskOutcomeOf(payload: AgentReportEventPayload): AgentReportTaskOutcome | null {
  const outcome = payload.task_report;
  if (outcome == null || typeof outcome !== "object") return null;
  return {
    persisted: outcome.persisted === true,
    transition_rejected: outcome.transition_rejected === true,
    plan_id: text(outcome.plan_id),
    task_id: text(outcome.task_id),
    cause: text(outcome.cause),
  };
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
    task: taskRef(payload.report, outcome),
    status: text(payload.report.status),
    branch: text(payload.report.branch),
    sha: text(payload.report.sha),
    taskOutcome: outcome,
    timestamp: typeof payload.report.timestamp === "number" ? payload.report.timestamp : null,
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

function ensureAgentReportListener(): void {
  if (eventSubscription) return;
  eventSubscription = listen<AgentReportEventPayload>(AGENT_REPORT_EVENT, (event) => {
    ingestAgentReport(event.payload);
  });
  void eventSubscription.catch(() => {
    eventSubscription = null;
  });
}

/** The subscription the hook uses; exported so non-React owners and fixtures
 *  drive the same listener ownership instead of a parallel one. */
export function subscribeAgentReportProjection(tabId: string, onChange: () => void): () => void {
  ensureAgentReportListener();
  const existing = listeners.get(tabId) ?? new Set<() => void>();
  existing.add(onChange);
  listeners.set(tabId, existing);
  return () => {
    existing.delete(onChange);
    if (existing.size === 0) listeners.delete(tabId);
  };
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
  eventSubscription = null;
  for (const tabId of affected) {
    for (const listener of listeners.get(tabId) ?? []) listener();
  }
}
