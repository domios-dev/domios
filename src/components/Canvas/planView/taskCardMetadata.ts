import type { BoardTaskCardModel } from "./boardModel";
import { formatSpan } from "../../../lib/timeLabels";
import { TaskStatus, type Task, type TaskStatus as TaskStatusType } from "../types";
import { middleTruncateSlug } from "./textTruncate";

export type TaskCardMetaRowKind = "handle" | "executor" | "historical" | "branch" | "age";

export interface TaskCardMetaRow {
  kind: TaskCardMetaRowKind;
  label: string;
  value: string;
  mono?: boolean;
}

export interface TaskCardMetadata {
  rows: TaskCardMetaRow[];
}

const TERMINAL: ReadonlySet<TaskStatusType> = new Set([TaskStatus.Done, TaskStatus.Cancelled]);

function activityTs(task: Task): number {
  let ts = task.status_changed_at ?? task.updated_at ?? task.created_at ?? 0;
  for (const m of task.milestones) if (m.ts > ts) ts = m.ts;
  return ts;
}

function branchRow(branch: string): TaskCardMetaRow {
  return { kind: "branch", label: "Branch", value: middleTruncateSlug(branch), mono: true };
}

function ageRow(task: Task, now: number): TaskCardMetaRow | null {
  const ts = activityTs(task);
  if (!ts) return null;
  return { kind: "age", label: "Age", value: formatSpan(now - ts) };
}

// Per-status secondary metadata — liveness chips live in workingOn only.
export function deriveTaskCardMetadata(
  card: BoardTaskCardModel,
  now: number = Date.now(),
): TaskCardMetadata {
  const { task, workingOn } = card;
  if (workingOn) return { rows: [] };

  const rows: TaskCardMetaRow[] = [];
  const handle = task.handle?.trim();
  const branch = card.branch;

  if (TERMINAL.has(task.status)) {
    if (card.historicalExecutorSummary) {
      rows.push({ kind: "historical", label: "Ran by", value: card.historicalExecutorSummary });
    }
    if (branch) rows.push(branchRow(branch));
    const age = ageRow(task, now);
    if (age) rows.push(age);
    return { rows };
  }

  if (handle) {
    rows.push({ kind: "handle", label: "Handle", value: handle, mono: true });
  } else if (card.presenceLabel) {
    const parts = [card.presenceLabel, card.roleLabel].filter(Boolean) as string[];
    rows.push({ kind: "executor", label: "Executor", value: parts.join(" · ") });
  } else if (card.historicalExecutorSummary) {
    rows.push({ kind: "historical", label: "Ran by", value: card.historicalExecutorSummary });
  }

  if (branch) rows.push(branchRow(branch));
  if (task.status === TaskStatus.Blocked) {
    const age = ageRow(task, now);
    if (age) rows.push(age);
  }

  return { rows };
}
