import type { Tab } from "../../lib/types";
import { isActiveTaskStatus } from "./tabCurrentTask";
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

function shortTabId(tabId: string): string {
  return tabId.length > 12 ? tabId.slice(0, 8) : tabId;
}

function tabLabel(tabId: string, tabs: Record<string, Tab>): string {
  return clean(tabs[tabId]?.title) ?? shortTabId(tabId);
}

export function attemptTabId(attempt: TaskExecutionAttempt): string | null {
  return clean(attempt.tab_id) ?? clean(attempt.executor_tab_id);
}

export function attemptTabTitle(attempt: TaskExecutionAttempt): string | null {
  return clean(attempt.tab_title) ?? clean(attempt.executor_title);
}

function attemptReportStatus(attempt: TaskExecutionAttempt): string | null {
  return clean(attempt.report_status) ?? clean(attempt.report?.status);
}

function normalizeStoredAttempt(
  attempt: TaskExecutionAttempt,
  tabs: Record<string, Tab>,
): TaskExecutionAttempt {
  const tabId = attemptTabId(attempt);
  const tabTitle = attemptTabTitle(attempt) ?? (tabId ? tabLabel(tabId, tabs) : null);
  const reportStatus = attemptReportStatus(attempt);
  return {
    ...attempt,
    tab_id: tabId,
    tab_title: tabTitle,
    report_status: reportStatus,
    outcome: clean(attempt.outcome) ?? reportStatus,
  };
}

function attemptLabel(attempt: TaskExecutionAttempt, tabs: Record<string, Tab>): string {
  const tabId = attemptTabId(attempt);
  return attemptTabTitle(attempt) ?? (tabId ? tabLabel(tabId, tabs) : "unknown");
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
  tabs: Record<string, Tab>,
): TaskExecutionAttempt[] {
  const stored = task.execution_attempts
    ?.filter((a) => attemptTabId(a) || attemptTabTitle(a))
    .map((a) => normalizeStoredAttempt(a, tabs));
  if (stored?.length) return stored;

  const tabId = clean(task.assigned_tab_id) ?? clean(task.lane?.tab_id);
  if (!tabId && !clean(task.taken_by_role)) return [];

  return [normalizeStoredAttempt({
    tab_id: tabId,
    tab_title: tabId ? tabs[tabId]?.title ?? null : null,
    agent_type: tabId ? tabs[tabId]?.agent_type ?? null : null,
    model: null,
    role: task.taken_by_role ?? null,
    branch: task.lane?.branch ?? null,
    worktree_path: task.lane?.worktree_path ?? null,
    report_status: inferReportStatus(task),
    closed_at: task.status_changed_at,
    outcome: task.status,
  }, tabs)];
}

export function buildHistoricalExecutorView(
  task: Task,
  tabs: Record<string, Tab>,
): HistoricalExecutorView | null {
  const attempts = resolveTaskExecutionAttempts(task, tabs);
  if (!attempts.length) return null;

  const primary = attempts[attempts.length - 1];
  const label = attemptLabel(primary, tabs);
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
  tabs: Record<string, Tab>,
  hasActivePresence: boolean,
): HistoricalExecutorView | null {
  if (hasActivePresence || isActiveTaskStatus(task.status)) return null;
  return buildHistoricalExecutorView(task, tabs);
}
