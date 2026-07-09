import type { Pane } from "../../lib/types";
import { isActiveTaskStatus } from "./paneCurrentTask";
import type { Milestone, Task, TaskExecutionAttempt } from "./types";

export interface HistoricalExecutorView {
  label: string;
  role: string | null;
  model: string | null;
  branch: string | null;
  reportStatus: string | null;
  outcome: string | null;
  attemptCount: number;
  attempts: TaskExecutionAttempt[];
  summary: string;
}

const EMPTY_LABELS = new Set(["", "unknown"]);

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text && !EMPTY_LABELS.has(text.toLowerCase()) ? text : null;
}

function shortPaneId(paneId: string): string {
  return paneId.length > 12 ? paneId.slice(0, 8) : paneId;
}

function paneLabel(paneId: string, panes: Record<string, Pane>): string {
  return clean(panes[paneId]?.title) ?? shortPaneId(paneId);
}

export function attemptPaneId(attempt: TaskExecutionAttempt): string | null {
  return clean(attempt.pane_id) ?? clean(attempt.executor_pane_id);
}

export function attemptPaneTitle(attempt: TaskExecutionAttempt): string | null {
  return clean(attempt.pane_title) ?? clean(attempt.executor_title);
}

function attemptReportStatus(attempt: TaskExecutionAttempt): string | null {
  return clean(attempt.report_status) ?? clean(attempt.report?.status);
}

function normalizeStoredAttempt(
  attempt: TaskExecutionAttempt,
  panes: Record<string, Pane>,
): TaskExecutionAttempt {
  const paneId = attemptPaneId(attempt);
  const paneTitle = attemptPaneTitle(attempt) ?? (paneId ? paneLabel(paneId, panes) : null);
  const reportStatus = attemptReportStatus(attempt);
  return {
    ...attempt,
    pane_id: paneId,
    pane_title: paneTitle,
    report_status: reportStatus,
    outcome: clean(attempt.outcome) ?? reportStatus,
  };
}

function attemptLabel(attempt: TaskExecutionAttempt, panes: Record<string, Pane>): string {
  const paneId = attemptPaneId(attempt);
  return attemptPaneTitle(attempt) ?? (paneId ? paneLabel(paneId, panes) : "unknown");
}

function inferReportStatus(task: Task): string | null {
  const reportMilestone = task.milestones
    .filter((m) => /report|status|done|blocked|partial|review/i.test(m.kind))
    .reduce<Milestone | null>((latest, m) => (!latest || m.ts > latest.ts ? m : latest), null);
  if (reportMilestone) return reportMilestone.kind.replace(/[_-]+/g, " ");
  if (!isActiveTaskStatus(task.status)) return task.status.replace(/_/g, " ");
  return null;
}

export function resolveTaskExecutionAttempts(
  task: Task,
  panes: Record<string, Pane>,
): TaskExecutionAttempt[] {
  const stored = task.execution_attempts
    ?.filter((a) => attemptPaneId(a) || attemptPaneTitle(a))
    .map((a) => normalizeStoredAttempt(a, panes));
  if (stored?.length) return stored;

  const paneId = clean(task.assigned_pane_id) ?? clean(task.lane?.pane_id);
  if (!paneId && !clean(task.taken_by_role)) return [];

  return [normalizeStoredAttempt({
    pane_id: paneId,
    pane_title: paneId ? panes[paneId]?.title ?? null : null,
    agent_type: paneId ? panes[paneId]?.agent_type ?? null : null,
    model: task.model,
    role: task.taken_by_role ?? null,
    branch: task.lane?.branch ?? null,
    worktree_path: task.lane?.worktree_path ?? null,
    report_status: inferReportStatus(task),
    closed_at: task.status_changed_at,
    outcome: task.status,
  }, panes)];
}

export function buildHistoricalExecutorView(
  task: Task,
  panes: Record<string, Pane>,
): HistoricalExecutorView | null {
  const attempts = resolveTaskExecutionAttempts(task, panes);
  if (!attempts.length) return null;

  const primary = attempts[attempts.length - 1];
  const label = attemptLabel(primary, panes);
  const role = clean(primary.role);
  const model = clean(primary.model);
  const branch = clean(primary.branch);
  const reportStatus = clean(primary.report_status);
  const outcome = clean(primary.outcome);
  const parts = [label, role, model, branch, reportStatus].filter(Boolean) as string[];

  return {
    label,
    role,
    model,
    branch,
    reportStatus,
    outcome,
    attemptCount: attempts.length,
    attempts,
    summary: parts.join(" · "),
  };
}

export function historicalExecutorForTask(
  task: Task,
  panes: Record<string, Pane>,
  hasActivePresence: boolean,
): HistoricalExecutorView | null {
  if (hasActivePresence || isActiveTaskStatus(task.status)) return null;
  return buildHistoricalExecutorView(task, panes);
}
