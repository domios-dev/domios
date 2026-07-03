import type { KeyboardEvent } from "react";
import { GitBranch, Hash, History, User } from "lucide-react";
import { TASK_STATUS_COLORS } from "../settings";
import type { BoardTaskCardModel } from "./boardModel";
import { usePlanTaskActivate } from "./planTaskInteraction";

function statusText(task: BoardTaskCardModel["task"]): string {
  return task.status_label?.trim() || task.status.replace(/_/g, " ");
}

export interface BoardTaskCardProps {
  card: BoardTaskCardModel;
  onOpen?: () => void;
}

export function BoardTaskCard({ card, onOpen }: BoardTaskCardProps) {
  const { task } = card;
  const statusColor = TASK_STATUS_COLORS[task.status];
  const handle = task.handle?.trim() || null;
  const interactive = Boolean(onOpen);
  const { onPointerDown, onPointerMove, onClick } = usePlanTaskActivate(onOpen);

  const onKeyDown = (e: KeyboardEvent) => {
    if (!onOpen) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onOpen();
    }
  };

  return (
    <article
      data-testid="plan-board-task-card"
      data-task-id={task.id}
      data-task-status={task.status}
      className={`plan-board-task-card plan-task-selectable${interactive ? " plan-board-task-card-interactive" : ""}`}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onPointerDown={interactive ? onPointerDown : undefined}
      onPointerMove={interactive ? onPointerMove : undefined}
      onClick={interactive ? onClick : undefined}
      onKeyDown={interactive ? onKeyDown : undefined}
      style={{
        borderColor: card.focusMatch
          ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 45%, var(--ct-border, #30363d))"
          : undefined,
      }}
    >
      <h4 data-testid="plan-board-task-title" className="plan-board-task-title">
        {task.name}
      </h4>
      <span
        data-testid="plan-board-task-status"
        className="plan-board-task-status"
        style={{ background: statusColor }}
      >
        {statusText(task)}
      </span>
      {handle && (
        <p data-testid="plan-board-task-handle" className="plan-board-task-meta plan-board-task-handle">
          <span className="plan-board-task-meta-icon" aria-hidden>
            <Hash size={9} strokeWidth={2.5} />
          </span>
          <span className="min-w-0 truncate">{handle}</span>
        </p>
      )}
      {card.presenceLabel && (
        <p data-testid="plan-board-task-active-executor" className="plan-board-task-meta">
          <span className="plan-board-task-meta-icon" aria-hidden>
            <User size={11} className="opacity-70" />
          </span>
          <span className="min-w-0 break-words">
            {[card.presenceLabel, card.roleLabel].filter(Boolean).join(" · ")}
          </span>
        </p>
      )}
      {card.historicalExecutorSummary && !card.presenceLabel && (
        <p data-testid="plan-board-task-historical-executor" className="plan-board-task-meta plan-board-task-historical">
          <span className="plan-board-task-meta-icon" aria-hidden>
            <History size={11} className="opacity-60" />
          </span>
          <span className="min-w-0 break-words">{card.historicalExecutorSummary}</span>
        </p>
      )}
      {card.branch && (
        <p data-testid="plan-board-task-lane" className="plan-board-task-meta plan-board-task-lane">
          <span className="plan-board-task-meta-icon" aria-hidden>
            <GitBranch size={11} className="opacity-70" />
          </span>
          <span className="min-w-0 truncate">{card.branch}</span>
        </p>
      )}
      {card.activityHint && (
        <p data-testid="plan-board-task-activity" className="plan-board-task-meta plan-board-task-activity">
          <span className="plan-board-task-meta-icon" aria-hidden />
          <span className="plan-board-task-activity-text">
            <span className="plan-board-task-activity-kind">{card.activityHint.kind}</span>
            <span className="plan-board-task-activity-age">{card.activityHint.ageLabel}</span>
          </span>
        </p>
      )}
    </article>
  );
}
