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

const TASK_HEADER = /^task:\s*\S+\s*$/i;

function exactPayload(reportText: string): string | null {
  const lines = reportText.split(/\r?\n/);
  const bodyStart = TASK_HEADER.test(lines[0]) ? 1 : 0;
  const payload = lines.slice(bodyStart).join("\n");
  return payload.trim() === payload && payload.startsWith("{") && payload.endsWith("}") ? payload : null;
}

function isSupportedEnvelope(kind: string, payload: Record<string, unknown>): boolean {
  if (!COORDINATION_KINDS.has(kind)) return false;
  if (typeof payload.plan_id !== "string" || typeof payload.task_id !== "string") return false;
  if (kind === "dispatch_ready") {
    return typeof payload.tab_id === "string" && typeof payload.attempt_id === "string" && Boolean(payload.lane) && typeof payload.lane === "object";
  }
  if (kind === "worker_ack") {
    return typeof payload.tab_id === "string" && typeof payload.attempt_id === "string" && (payload.state === "in_progress" || payload.state === "awaiting_runtime_ready");
  }
  return true;
}

export function parseCoordinationReport(reportText: string): CoordinationReportDescriptor | null {
  const rawPayload = exactPayload(reportText);
  if (!rawPayload) return null;
  let payload: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(rawPayload);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    payload = parsed as Record<string, unknown>;
  } catch {
    return null;
  }
  const kind = typeof payload.kind === "string" ? payload.kind : undefined;
  if (!kind || !isSupportedEnvelope(kind, payload)) return null;
  const planId = displayText(payload.plan_id, 80);
  const taskId = displayText(payload.task_id, 80);
  return {
    event: eventLabel(kind),
    state: actionableState(kind, payload),
    task: planId && taskId ? `${planId}/${taskId}` : taskId,
    diagnostics: reportText,
  };
}
