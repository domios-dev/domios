import type { GraphWorkerInfo } from "../../lib/types";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";
import { formatDuration } from "../../lib/timeLabels";

export function workerFailureReason(worker: GraphWorkerInfo): string | null {
  const end = worker.execution_end;
  if (end?.kind === "process_ended" || end?.kind === "timed_out") return end.detail;
  if (worker.status === "error" || worker.status === "orphaned") {
    return worker.report ?? (worker.status === "orphaned" ? "worker orphaned; end time unknown" : "failure reason unavailable");
  }
  return null;
}

export function workerDurationMs(worker: GraphWorkerInfo, now: number): number | null {
  const end = worker.status === "orphaned"
    ? worker.last_observed_at
    : isTerminalWorkerStatus(worker.status) ? worker.done_at : now;
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
  const recent = workers.filter((worker) => isTerminalWorkerStatus(worker.status))
    .sort((a, b) => b.spawned_at - a.spawned_at);
  for (const worker of recent) {
    const failed = worker.status === "error" || worker.status === "orphaned";
    const key = failed
      ? JSON.stringify([worker.intent, worker.status, worker.agent_provider, worker.model, workerFailureReason(worker)])
      : worker.worker_id;
    const existing = groups.get(key);
    if (existing) existing.count++;
    else groups.set(key, { worker, count: 1 });
  }
  return [...groups.values()];
}
