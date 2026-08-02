import { decodeCliMessageText } from "./messageText";

export interface CoordinationReportDescriptor {
  event: string;
  state: string;
  task?: string;
  diagnostics: string;
}

const COORDINATION_KINDS = new Set([
  "dispatch_ready",
  "worker_ack",
  "dispatch_startup_failed",
  "startup_failed",
  "worker_ack_timeout",
  "worker_ack_tracking_failed",
  "orchestration_incident",
]);

function displayText(value: unknown, limit: number): string | undefined {
  if (typeof value !== "string") return undefined;
  const decoded = decodeCliMessageText(value).trim();
  if (!decoded) return undefined;
  return decoded.length <= limit ? decoded : `${decoded.slice(0, limit - 1)}…`;
}

function eventLabel(kind: string): string {
  const labels: Record<string, string> = {
    dispatch_ready: "Dispatch ready",
    worker_ack: "Worker acknowledged",
    dispatch_startup_failed: "Worker startup failed",
    startup_failed: "Worker startup failed",
    worker_ack_timeout: "Worker acknowledgement overdue",
    worker_ack_tracking_failed: "Worker acknowledgement tracking failed",
    orchestration_incident: "Orchestration incident",
  };
  return labels[kind] ?? kind.replace(/_/g, " ");
}

function actionableState(kind: string, payload: Record<string, unknown>): string {
  if (kind === "dispatch_ready") return "Awaiting worker acknowledgement";
  if (kind === "worker_ack") {
    return payload.state === "awaiting_runtime_ready" ? "Awaiting runtime readiness" : "In progress";
  }
  if (kind === "dispatch_startup_failed" || kind === "startup_failed") return "Retry required";
  if (kind === "worker_ack_timeout") return "Inspect the existing worker";
  if (kind === "worker_ack_tracking_failed") return "Inspect acknowledgement tracking";
  return "Needs attention";
}

const TASK_HEADER = /^task:\s*([^/\s]+)\/([^/\s]+)\s*$/i;

interface ExactEnvelope {
  rawPayload: string;
  headerPlanId?: string;
  headerTaskId?: string;
}

function exactEnvelope(reportText: string): ExactEnvelope | null {
  const lines = reportText.split(/\r?\n/);
  const header = TASK_HEADER.exec(lines[0]);
  const rawPayload = lines.slice(header ? 1 : 0).join("\n");
  if (rawPayload.trim() !== rawPayload || !rawPayload.startsWith("{") || !rawPayload.endsWith("}")) return null;
  return { rawPayload, headerPlanId: header?.[1], headerTaskId: header?.[2] };
}

function isSupportedEnvelope(kind: string, payload: Record<string, unknown>): boolean {
  if (!COORDINATION_KINDS.has(kind)) return false;
  if (kind === "dispatch_ready") {
    return typeof payload.tab_id === "string"
      && typeof payload.attempt_id === "string"
      && typeof payload.session_id === "string"
      && (payload.lane === undefined || Boolean(payload.lane) && typeof payload.lane === "object" && !Array.isArray(payload.lane));
  }
  if (kind === "worker_ack") {
    return typeof payload.tab_id === "string" && typeof payload.attempt_id === "string" && (payload.state === "in_progress" || payload.state === "awaiting_runtime_ready");
  }
  if (kind === "dispatch_startup_failed" || kind === "startup_failed") {
    return typeof payload.stage === "string" && typeof payload.error === "string";
  }
  return true;
}

export function parseCoordinationReport(reportText: string): CoordinationReportDescriptor | null {
  const envelope = exactEnvelope(reportText);
  if (!envelope) return null;
  let payload: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(envelope.rawPayload);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    payload = parsed as Record<string, unknown>;
  } catch {
    return null;
  }
  const kind = typeof payload.kind === "string" ? payload.kind : undefined;
  if (!kind || !isSupportedEnvelope(kind, payload)) return null;
  const payloadPlanId = typeof payload.plan_id === "string" ? payload.plan_id : undefined;
  const payloadTaskId = typeof payload.task_id === "string" ? payload.task_id : undefined;
  if ((payloadPlanId == null) !== (payloadTaskId == null)) return null;
  if (payloadPlanId && envelope.headerPlanId && (payloadPlanId !== envelope.headerPlanId || payloadTaskId !== envelope.headerTaskId)) return null;
  const mayUseHeaderIdentity = kind === "dispatch_startup_failed" || kind === "startup_failed";
  const rawPlanId = payloadPlanId ?? (mayUseHeaderIdentity ? envelope.headerPlanId : undefined);
  const rawTaskId = payloadTaskId ?? (mayUseHeaderIdentity ? envelope.headerTaskId : undefined);
  if (!rawPlanId || !rawTaskId) return null;
  const planId = displayText(rawPlanId, 80);
  const taskId = displayText(rawTaskId, 80);
  return {
    event: eventLabel(kind),
    state: actionableState(kind, payload),
    task: planId && taskId ? `${planId}/${taskId}` : taskId,
    diagnostics: reportText,
  };
}
