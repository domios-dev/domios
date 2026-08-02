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

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
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
  const explicit = text(payload.state) ?? text(payload.impact) ?? text(payload.error);
  if (explicit) return explicit.replace(/_/g, " ");
  if (kind === "dispatch_ready") return "Awaiting worker acknowledgement";
  if (kind === "worker_ack") return "In progress";
  if (kind.endsWith("startup_failed")) return "Needs attention";
  return "Inspect details";
}

export function parseCoordinationReport(reportText: string): CoordinationReportDescriptor | null {
  const diagnostics = decodeCliMessageText(reportText);
  const bodyStart = diagnostics.split("\n").findIndex((line) => line.trimStart().startsWith("{"));
  if (bodyStart < 0) return null;
  const rawPayload = diagnostics.split("\n").slice(bodyStart).join("\n").trim();
  let payload: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(rawPayload);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return null;
    payload = parsed as Record<string, unknown>;
  } catch {
    return null;
  }
  const kind = text(payload.kind);
  if (!kind || !COORDINATION_KINDS.has(kind)) return null;
  const planId = text(payload.plan_id);
  const taskId = text(payload.task_id);
  return {
    event: eventLabel(kind),
    state: actionableState(kind, payload),
    task: planId && taskId ? `${planId}/${taskId}` : taskId,
    diagnostics,
  };
}
