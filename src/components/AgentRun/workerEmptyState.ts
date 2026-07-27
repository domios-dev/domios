export const TERMINAL_STATUSES = new Set([
  "done",
  "error",
  "failed",
  "cancelled",
  "orphaned",
  "hard_timeout",
  "timeout",
  "timed_out",
]);

function normalizeStatus(status: string): string {
  return status.toLowerCase().replaceAll("-", "_").replaceAll(" ", "_");
}

export function isTerminalWorkerStatus(status: string): boolean {
  return TERMINAL_STATUSES.has(normalizeStatus(status));
}

export function workerEmptyState(
  liveTabId: string | null,
  connected: boolean,
  status: string,
  reason?: string | null,
): string {
  if (liveTabId) return connected ? "Waiting for first event…" : "Connecting…";
  const s = normalizeStatus(status);
  if (TERMINAL_STATUSES.has(s)) {
    if (s === "done") return "Completed — no output captured";
    const detail = reason?.trim() ? ` — ${reason.trim()}` : "";
    if (s === "orphaned") return `Worker orphaned — no active session${detail}`;
    if (s === "cancelled") return `Worker cancelled${detail}`;
    if (s === "hard_timeout" || s === "timeout" || s === "timed_out") {
      return `Worker hard timeout${detail}`;
    }
    return `Worker failed${detail}`;
  }
  if (s === "queued") return "Queued — waiting for worker slot…";
  return "Waiting for spawn…";
}
