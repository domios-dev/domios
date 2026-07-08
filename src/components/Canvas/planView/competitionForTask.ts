import type { Pane } from "../../../lib/types";
import type { Task, TaskExecutionAttempt } from "../types";
import { attemptPaneId, attemptPaneTitle } from "../taskExecutorHistory";

export interface CompetitionForTaskResult {
  count: number;
  liveCount: number;
  tooltip: string;
}

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text || null;
}

export function isCompetitiveTask(task: Task): boolean {
  return task.competition?.enabled === true;
}

export function competitionAttempts(task: Task): number {
  return Math.max(2, task.competition?.attempts ?? 2);
}

export function liveCompetitionAttempts(task: Task): TaskExecutionAttempt[] {
  return (task.execution_attempts ?? []).filter(
    (attempt) => clean(attempt.competition_group_id) && attempt.closed_at == null,
  );
}

export function liveCompetitionAttemptCount(task: Task): number {
  return liveCompetitionAttempts(task).length;
}

function attemptLabel(attempt: TaskExecutionAttempt, panes: Record<string, Pane>): string {
  const paneId = attemptPaneId(attempt);
  return attemptPaneTitle(attempt) ?? (paneId ? clean(panes[paneId]?.title) ?? paneId.slice(0, 8) : "unknown");
}

export function competitionForTask(
  task: Task,
  panes: Record<string, Pane> = {},
): CompetitionForTaskResult | null {
  if (!isCompetitiveTask(task)) return null;

  const count = competitionAttempts(task);
  const live = liveCompetitionAttempts(task);
  const lines = [`${count} competitive attempts configured`];

  const models = task.competition?.models?.map((model) => model.trim()).filter(Boolean) ?? [];
  if (models.length) lines.push(`models: ${models.join(", ")}`);

  const judgeHint = clean(task.competition?.judge_hint);
  if (judgeHint) lines.push(`judge: ${judgeHint}`);

  if (!live.length) {
    lines.push("no live attempts");
  } else {
    for (const attempt of live) {
      const label = attemptLabel(attempt, panes);
      const model = clean(attempt.model);
      const lane = attempt.competition_lane != null ? ` lane ${attempt.competition_lane}` : "";
      lines.push(`live: ${label}${lane}${model ? ` · ${model}` : ""}`);
    }
  }

  return {
    count,
    liveCount: live.length,
    tooltip: lines.join("\n"),
  };
}
