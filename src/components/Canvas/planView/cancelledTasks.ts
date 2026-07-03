import { TaskStatus, type Plan, type Task } from "../types";

export function listCancelledTasks(plan: Plan): Task[] {
  return plan.tasks.filter((t) => t.status === TaskStatus.Cancelled);
}

export function cancelledTaskCount(plan: Plan): number {
  return listCancelledTasks(plan).length;
}
