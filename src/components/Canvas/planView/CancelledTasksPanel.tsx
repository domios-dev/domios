import { Hash } from "lucide-react";
import type { Task } from "../types";

export interface CancelledTasksPanelProps {
  tasks: readonly Task[];
  onOpenTask: (task: Task) => void;
}

export function CancelledTasksPanel({ tasks, onOpenTask }: CancelledTasksPanelProps) {
  if (tasks.length === 0) return null;

  return (
    <ul data-testid="plan-cancelled-tasks-list" className="plan-cancelled-tasks-list">
      {tasks.map((task) => {
        const handle = task.handle?.trim() || null;
        return (
          <li key={task.id} className="plan-cancelled-tasks-item">
            <button
              type="button"
              data-testid="plan-cancelled-task-row"
              data-task-id={task.id}
              className="plan-cancelled-task-row plan-task-selectable"
              onClick={() => onOpenTask(task)}
            >
              {handle && (
                <span data-testid="plan-cancelled-task-handle" className="plan-cancelled-task-handle">
                  <Hash size={9} strokeWidth={2.5} aria-hidden />
                  {handle}
                </span>
              )}
              <span data-testid="plan-cancelled-task-title" className="plan-cancelled-task-title">
                {task.name}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
