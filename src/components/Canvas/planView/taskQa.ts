import type { Plan, Task } from "../types";

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
  return position ? `In QA · queued #${position}` : "In QA · queued";
}

export function taskStatusText(plan: Plan, task: Task): string {
  return qaChipText(plan, task) ?? task.status_label?.trim() ?? task.status.replace(/_/g, " ");
}
