import type { Plan, Task } from "../types";
import { canonicalTaskStatusLabel } from "./taskStatusLabel";

export function qaQueuePosition(plan: Plan, task: Task): number | null {
  if (task.status !== "in_review" || task.review_stage !== "qa" || task.qa_state !== "queued") return null;
  let position = 0;
  for (const candidate of plan.tasks) {
    if (candidate.status === "in_review" && candidate.review_stage === "qa" && candidate.qa_state === "queued") {
      position += 1;
      if (candidate.id === task.id) return position;
    }
  }
  return null;
}

export function qaChipText(plan: Plan, task: Task): string | null {
  if (task.status !== "in_review" || task.review_stage !== "qa") return null;
  if (task.qa_state === "running") return "In QA · running";
  const position = qaQueuePosition(plan, task);
  const base = position ? `In QA · queued #${position}` : "In QA · queued";
  const reason = task.status_label?.trim();
  return reason ? `${base} — ${reason}` : base;
}

export function taskStatusText(_plan: Plan, task: Task): string {
  return canonicalTaskStatusLabel(task);
}
