import type { KeyboardEvent } from "react";
import { ChevronRight, GitBranch, Hash, History, Layers, User } from "lucide-react";
import { TASK_STATUS_COLORS } from "../settings";
import type { BoardTaskCardModel } from "./boardModel";
import { usePlanTaskActivate } from "./planTaskInteraction";
import { TaskExecutionBadges } from "./TaskExecutionBadges";
import { WorkingOnRow } from "./WorkingOnRowView";
import { deriveTaskCardMetadata, type TaskCardMetaRow } from "./taskCardMetadata";
import { TaskStatus } from "../types";
import { taskDisplayKey, taskReferenceLabel } from "../taskDisplayKey";
import {
  MissionControlControls,
  type MissionControlTaskAction,
  type MissionControlTaskPauseAction,
} from "../missionControl/MissionControlControls";

export interface BoardTaskActions {
  onPauseTask?: MissionControlTaskPauseAction;
  onResumeTask?: MissionControlTaskAction;
  onDeleteTask?: MissionControlTaskAction;
}

export interface BoardTaskCardProps extends BoardTaskActions {
  card: BoardTaskCardModel;
  onOpen?: () => void;
  planId?: string;
}

function metaIcon(kind: TaskCardMetaRow["kind"]) {
  const size = 11;
  if (kind === "handle") return <Hash size={9} strokeWidth={2.5} />;
  if (kind === "branch") return <GitBranch size={size} className="opacity-70" />;
  if (kind === "historical") return <History size={size} className="opacity-60" />;
  return <User size={size} className="opacity-70" />;
}

function metaTestId(kind: TaskCardMetaRow["kind"]): string {
  if (kind === "handle") return "plan-board-task-handle";
  if (kind === "branch") return "plan-board-task-lane";
  if (kind === "historical") return "plan-board-task-historical-executor";
  if (kind === "executor") return "plan-board-task-active-executor";
  return "plan-board-task-age";
}

export function BoardTaskCard({ card, onOpen, planId, onPauseTask, onResumeTask, onDeleteTask }: BoardTaskCardProps) {
  const { task } = card;
  const statusColor = TASK_STATUS_COLORS[task.status];
  const isPaused = task.status === TaskStatus.Paused;
  const pauseReason = task.pause_reason?.trim() || null;
  const pausedTitle = isPaused
    ? `Paused${pauseReason ? `: ${pauseReason}` : ""}`
    : undefined;
  const interactive = Boolean(onOpen);
  const metadata = deriveTaskCardMetadata(card);
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
      className={`plan-board-task-card plan-task-selectable${interactive ? " plan-board-task-card-interactive" : ""}${isPaused ? " plan-board-task-card-paused" : ""}`}
      role={interactive ? "button" : undefined}
      aria-label={interactive ? `Open task ${taskReferenceLabel(task)}` : undefined}
      tabIndex={interactive ? 0 : undefined}
      title={pausedTitle}
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
      {planId && (
        <div className="plan-board-task-actions">
          <MissionControlControls
            scope="task"
            planId={planId}
            taskId={task.id}
            taskName={task.name}
            taskStatus={task.status}
            onPauseTask={onPauseTask}
            onResumeTask={onResumeTask}
            onDeleteTask={onDeleteTask}
          />
        </div>
      )}
      <p data-testid="plan-board-task-key" className="plan-board-task-key plan-board-task-mono">
        {taskDisplayKey(task)}
      </p>
      <h4 data-testid="plan-board-task-title" className="plan-board-task-title">
        {task.name}
      </h4>
      {card.locator && (
        <div data-testid="plan-board-task-locator" className="plan-board-task-locator">
          {card.locator.groupLabel && (
            <span
              className="plan-board-task-locator-group"
              data-testid="plan-board-task-locator-group"
              title={`Group: ${card.locator.groupLabel}`}
            >
              <span
                className="plan-board-task-locator-dot"
                style={{ background: card.locator.groupColor ?? "var(--ct-accent, #58a6ff)" }}
                aria-hidden
              />
              <span className="plan-board-task-locator-group-label">{card.locator.groupLabel}</span>
            </span>
          )}
          {card.locator.groupLabel && card.locator.section && (
            <ChevronRight
              size={9}
              className="plan-board-task-locator-sep"
              aria-hidden
            />
          )}
          {card.locator.section && (
            <span
              className="plan-board-task-locator-section"
              data-testid="plan-board-task-locator-section"
              title={`Stage: ${card.locator.section}`}
            >
              <Layers size={9} className="plan-board-task-locator-section-icon" aria-hidden />
              <span className="min-w-0 truncate">{card.locator.section}</span>
            </span>
          )}
        </div>
      )}
      <TaskExecutionBadges loop={card.loopBadge} competition={card.competeBadge} />
      <span
        data-testid="status-chip"
        className={`plan-board-task-status${isPaused ? " plan-board-task-status-paused" : ""}`}
        style={{ background: statusColor }}
        title={pausedTitle}
      >
        {card.statusChipText}
      </span>
      {card.workingOn ? (
        <WorkingOnRow model={card.workingOn} variant="board" />
      ) : (
        metadata.rows.map((row) => (
          <p
            key={row.kind}
            data-testid={metaTestId(row.kind)}
            className={`plan-board-task-meta plan-board-task-meta-${row.kind}${row.mono ? " plan-board-task-mono" : ""}`}
          >
            <span className="plan-board-task-meta-icon" aria-hidden>
              {metaIcon(row.kind)}
            </span>
            <span className="plan-board-task-meta-label">{row.label}</span>
            <span className="plan-board-task-meta-value min-w-0 truncate">{row.value}</span>
          </p>
        ))
      )}
      {card.latestNote && (
        <p data-testid="latest-note" className="plan-board-task-latest-note">
          {card.latestNote}
        </p>
      )}
    </article>
  );
}
