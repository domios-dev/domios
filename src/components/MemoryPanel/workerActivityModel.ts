import type { GraphWorkerInfo } from "../../lib/types";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";
import { formatDuration } from "../../lib/timeLabels";

export function workerFailureReason(worker: GraphWorkerInfo): string | null {
  if (worker.extraction_retry?.reason) return worker.extraction_retry.reason;
  const end = worker.execution_end;
  if (end?.kind === "process_ended" || end?.kind === "timed_out") return end.detail;
  if (worker.status === "error" || worker.status === "orphaned") {
    return (
      worker.report ??
      (worker.status === "orphaned"
        ? "worker orphaned; end time unknown"
        : "failure reason unavailable")
    );
  }
  return null;
}

export type MemoryAgentKind = "extraction" | "dreaming" | "custom";

export function workerAgentKind(worker: GraphWorkerInfo): MemoryAgentKind {
  if (worker.intent.startsWith("extract:")) return "extraction";
  if (worker.intent.startsWith("dreaming:")) return "dreaming";
  return "custom";
}

export function workerTaskLabel(worker: GraphWorkerInfo, title?: string): string {
  switch (workerAgentKind(worker)) {
    case "extraction":
      return title ? `Extract knowledge from ${title}` : "Extract knowledge from a saved memory";
    case "dreaming":
      return "Consolidate memories and write a dream diary";
    default:
      return worker.intent.replace(/[_:]+/g, " ").trim() || "Memory task";
  }
}

export function workerOutcome(worker: GraphWorkerInfo): string {
  if (["error", "failed"].includes(worker.status)) return "Failed";
  if (["hard_timeout", "timeout", "timed_out"].includes(worker.status)) return "Timed out";
  if (worker.status === "cancelled") return "Cancelled";
  if (worker.status === "orphaned") return "Interrupted";
  if (!isTerminalWorkerStatus(worker.status))
    return worker.status === "starting" ? "Starting" : "Running";
  if (worker.finalization?.state === "failed") return "Diary failed";
  if (worker.finalization?.state === "partial") return "Diary incomplete";
  if (workerAgentKind(worker) === "dreaming" && worker.finalization?.state !== "succeeded")
    return "Unverified";
  return worker.status === "done" ? "Completed" : worker.status;
}

export function workerNeedsAttention(worker: GraphWorkerInfo): boolean {
  return ["Failed", "Interrupted", "Timed out", "Diary failed", "Diary incomplete"].includes(
    workerOutcome(worker),
  );
}

export function agentActivity(workers: GraphWorkerInfo[]) {
  const sorted = [...workers].sort((a, b) => b.spawned_at - a.spawned_at);
  const live = sorted.filter((worker) => !isTerminalWorkerStatus(worker.status));
  const latest = live[0] ?? sorted[0];
  const parked = sorted.filter(
    (worker, index) =>
      worker.extraction_retry?.parked &&
      sorted.findIndex((candidate) => candidate.intent === worker.intent) === index,
  );
  return { latest, live, parked, history: sorted };
}

export function workerDurationMs(worker: GraphWorkerInfo, now: number): number | null {
  const end =
    worker.status === "orphaned"
      ? worker.last_observed_at
      : isTerminalWorkerStatus(worker.status)
        ? worker.done_at
        : now;
  return end == null ? null : Math.max(0, end - worker.spawned_at);
}

export function formatWorkerTime(ms: number | null): string {
  return ms === null ? "unknown" : formatDuration(ms);
}

export function workerTimeLabels(worker: GraphWorkerInfo, now: number) {
  return {
    age: `Age ${formatWorkerTime(now - worker.spawned_at)}`,
    duration: `Duration ${formatWorkerTime(workerDurationMs(worker, now))}`,
  };
}

export interface WorkerFailureGroup {
  worker: GraphWorkerInfo;
  count: number;
}

export function groupRecentWorkers(workers: GraphWorkerInfo[]): WorkerFailureGroup[] {
  const groups = new Map<string, WorkerFailureGroup>();
  const recent = workers
    .filter((worker) => isTerminalWorkerStatus(worker.status))
    .sort((a, b) => b.spawned_at - a.spawned_at);
  for (const worker of recent) {
    const failed = worker.status === "error" || worker.status === "orphaned";
    const key = failed
      ? JSON.stringify([
          worker.intent,
          worker.status,
          worker.agent_provider,
          worker.model,
          workerFailureReason(worker),
        ])
      : worker.worker_id;
    const existing = groups.get(key);
    if (existing) existing.count++;
    else groups.set(key, { worker, count: 1 });
  }
  return [...groups.values()];
}
