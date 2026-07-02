import type { KeyboardEvent } from "react";
import { GitBranch, Hash, History, User } from "lucide-react";
import { TASK_STATUS_COLORS } from "../settings";
import type { BoardTaskCardModel } from "./boardModel";

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
  const laneBits = [card.branch, card.worktree, card.workspaceName].filter(Boolean) as string[];
  const interactive = Boolean(onOpen);

  const activate = () => onOpen?.();

  const onKeyDown = (e: KeyboardEvent) => {
    if (!onOpen) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      activate();
    }
  };

  return (
    <article
      data-testid="plan-board-task-card"
      data-task-id={task.id}
      data-task-status={task.status}
      className={`plan-board-task-card${interactive ? " plan-board-task-card-interactive" : ""}`}
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onClick={interactive ? activate : undefined}
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
      <div className="plan-board-task-card-head">
        {handle && (
          <span data-testid="plan-board-task-handle" className="plan-board-task-handle">
            <Hash size={9} strokeWidth={2.5} aria-hidden />
            {handle}
          </span>
        )}
        <span
          data-testid="plan-board-task-status"
          className="plan-board-task-status"
          style={{ background: statusColor }}
        >
          {statusText(task)}
        </span>
      </div>
      {card.presenceLabel && (
        <p data-testid="plan-board-task-active-executor" className="plan-board-task-meta">
          <User size={11} aria-hidden className="shrink-0 opacity-70" />
          <span className="min-w-0 break-words">
            {[card.presenceLabel, card.roleLabel].filter(Boolean).join(" · ")}
          </span>
        </p>
      )}
      {card.historicalExecutorSummary && !card.presenceLabel && (
        <p data-testid="plan-board-task-historical-executor" className="plan-board-task-meta plan-board-task-historical">
          <History size={11} aria-hidden className="shrink-0 opacity-60" />
          <span className="min-w-0 break-words">{card.historicalExecutorSummary}</span>
        </p>
      )}
      {laneBits.length > 0 && (
        <p data-testid="plan-board-task-lane" className="plan-board-task-meta plan-board-task-lane">
          <GitBranch size={11} aria-hidden className="shrink-0 opacity-70" />
          <span className="min-w-0 break-words">{laneBits.join(" · ")}</span>
        </p>
      )}
      {card.latestMilestone && (
        <p data-testid="plan-board-task-milestone" className="plan-board-task-milestone">
          {card.latestMilestoneKind && (
            <span className="plan-board-task-milestone-kind">{card.latestMilestoneKind}</span>
          )}
          <span className="min-w-0 break-words">{card.latestMilestone}</span>
        </p>
      )}
    </article>
  );
}
