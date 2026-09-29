import type { Milestone, Plan, Task } from "../types";

const LATEST_NOTE_MAX = 120;

export function canonicalTaskStatusLabel(task: Task, plan?: Plan): string {
  if ((task.status === "pending" || task.status === "in_progress")
    && plan?.tasks.some((child) => child.parent_task_id === task.id && child.status !== "done" && child.status !== "cancelled")) {
    return "blocked on subtasks";
  }
  return task.status.replace(/_/g, " ");
}

export function latestMilestoneForTask(task: Task): Milestone | null {
  return task.milestones.reduce<Milestone | null>(
    (latest, milestone) => (!latest || milestone.ts > latest.ts ? milestone : latest),
    null,
  );
}

export function latestTaskNote(task: Task): string | null {
  const text = latestMilestoneForTask(task)?.text?.replace(/\s+/g, " ").trim();
  if (!text) return null;
  return text.length <= LATEST_NOTE_MAX ? text : `${text.slice(0, LATEST_NOTE_MAX - 1)}…`;
}
