import type { MissionTask } from "./types";

interface TaskCardProps {
  task: MissionTask;
  dense?: boolean;
}

export function TaskCard({ task, dense }: TaskCardProps) {
  return (
    <div className="mc-task-card" data-status={task.status} data-testid={`task-${task.id}`}>
      <div className="mc-task-title">{dense ? truncate(task.title, 28) : task.title}</div>
      <div className="mc-task-meta">
        <span className="mc-overview-only">{task.status}</span>
        <span className="mc-detail-only">
          {task.section ? `${task.section} · ` : ""}
          {task.status.replace("_", " ")}
          {task.workerName ? ` · ${task.workerName}` : ""}
        </span>
      </div>
    </div>
  );
}

function truncate(s: string, n: number): string {
  return s.length <= n ? s : `${s.slice(0, n - 1)}…`;
}
