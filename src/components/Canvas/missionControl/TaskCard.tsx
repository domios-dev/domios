import type { MissionTask } from "./types";

interface TaskCardProps {
  task: MissionTask;
  dense?: boolean;
  lod?: "far" | "mid" | "close";
  onClick?: (taskId: string) => void;
}

export function TaskCard({ task, dense, lod, onClick }: TaskCardProps) {
  const title = dense ? truncateWords(task.title, lod === "mid" ? 6 : 5) : task.title;
  return (
    <div
      className="mc-task-card"
      data-status={task.status}
      data-mc-interactive="true"
      data-testid={`task-${task.id}`}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onClick={onClick ? () => onClick(task.id) : undefined}
      onKeyDown={
        onClick
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick(task.id);
              }
            }
          : undefined
      }
    >
      <div className="mc-task-title" title={task.title}>
        {title}
      </div>
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

function truncateWords(s: string, maxWords: number): string {
  const words = s.trim().split(/\s+/);
  if (words.length <= maxWords) return s;
  return `${words.slice(0, maxWords).join(" ")}…`;
}
