import type { Task } from "../types";

export function taskFreshnessAt(task: Task): number | null {
  const ts = task.status_changed_at ?? task.updated_at ?? task.created_at;
  if (ts == null || !Number.isFinite(ts) || ts <= 0) return null;
  return ts;
}

export function formatTaskFreshness(ts: number, nowMs: number = Date.now()): string {
  if (!ts) return "";
  const seconds = Math.max(0, Math.round((nowMs - ts) / 1000));
  if (seconds < 45) return "now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days}d`;
  return new Date(ts).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
