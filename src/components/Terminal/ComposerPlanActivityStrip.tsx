import { ChevronDown, Map } from "lucide-react";
import { useId, useState } from "react";
import { TASK_STATUS_COLORS } from "../Canvas/settings";
import type { PlanAttachment, TaskStatus } from "../Canvas/types";
import type { PaneComposerActivity } from "../Canvas/paneComposerActivity";

export interface ComposerPlanActivityStripProps {
  activity: PaneComposerActivity;
  onOpen?: () => void;
  taskStatus?: TaskStatus | null;
  stale?: boolean;
  nextAction?: string;
  attachments?: readonly PlanAttachment[];
  detailText?: string;
}

function activityLabel(activity: PaneComposerActivity): { prefix: string; primary: string; secondary?: string } {
  if (activity.kind === "orchestrating") {
    return { prefix: "Orchestrating:", primary: activity.planTitle };
  }
  if (activity.kind === "planning") {
    return { prefix: "Planning:", primary: activity.planTitle };
  }
  return {
    prefix: "Working on:",
    primary: activity.taskTitle,
    secondary: activity.planTitle,
  };
}

export function ComposerPlanActivityStrip({
  activity,
  onOpen,
  taskStatus,
  stale = false,
  nextAction,
  attachments = [],
  detailText,
}: ComposerPlanActivityStripProps) {
  const [expanded, setExpanded] = useState(false);
  const detailsId = `composer-plan-details-${useId().replace(/:/g, "")}`;
  const { prefix, primary, secondary } = activityLabel(activity);
  const extraPlans = activity.kind === "orchestrating" ? activity.extraPlans : 0;
  const statusColor = taskStatus ? TASK_STATUS_COLORS[taskStatus] : null;
  const action = nextAction ?? (taskStatus === "blocked" ? "Resolve blocker" : "Continue active work");
  const summary = activity.kind === "working"
    ? `${prefix} ${primary}${secondary ? ` · ${secondary}` : ""}${taskStatus ? ` · ${taskStatus.replace(/_/g, " ")}` : ""}\nOpen task on canvas`
    : `${prefix} ${primary}${extraPlans > 0 ? ` (+${extraPlans} more plan${extraPlans === 1 ? "" : "s"})` : ""}\nOpen plan on canvas`;
  const fullText = [
    `${prefix} ${primary}`,
    secondary ? `Plan: ${secondary}` : null,
    taskStatus ? `Status: ${taskStatus.replace(/_/g, " ")}` : null,
    `Next action: ${action}`,
    detailText ? `Details:\n${detailText}` : null,
    attachments.length > 0 ? `Attachments: ${attachments.map((attachment) => attachment.name).join(", ")}` : null,
  ].filter(Boolean).join("\n");

  return (
    <div className="ct-composer-activity-capsule" data-expanded={expanded ? "true" : "false"}>
      <div className={`ct-composer-activity-chip h-7 max-w-full min-w-0${stale ? " ct-composer-activity-chip-stale" : ""}`}>
        <button
          type="button"
          data-testid="composer-plan-activity-strip"
          data-activity-kind={activity.kind}
          data-plan-id={activity.planId}
          data-task-id={activity.kind === "working" ? activity.taskId : undefined}
          data-stale={stale ? "true" : "false"}
          aria-label={summary.replace("\n", ". ")}
          title={summary}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onOpen}
          className="ct-composer-activity-chip ct-composer-activity-chip__open"
          style={{ cursor: onOpen ? "pointer" : "default" }}
        >
          <span data-testid="composer-plan-activity-prefix" className="ct-composer-activity-chip__prefix">{prefix}</span>
          <span data-testid="composer-plan-activity-primary" className="ct-composer-activity-chip__primary">{primary}</span>
          {activity.kind === "working" && taskStatus && (
            <span data-testid="composer-current-task" className="ct-composer-activity-chip__status" style={{ color: statusColor ?? undefined }}>
              {taskStatus.replace(/_/g, " ")}
            </span>
          )}
          {secondary && <><span aria-hidden className="ct-composer-activity-chip__sep">·</span><span data-testid="composer-plan-activity-secondary" className="ct-composer-activity-chip__secondary">{secondary}</span></>}
          {extraPlans > 0 && <span data-testid="composer-plan-activity-extra-plans" className="ct-composer-activity-chip__extra">+{extraPlans}</span>}
          {onOpen && <Map size={11} aria-hidden className="ct-composer-activity-chip__icon" />}
        </button>
        <span className="ct-composer-activity-chip__next" data-testid="composer-next-action">Next: {action}</span>
        {attachments.map((attachment) => (
          <span key={attachment.id} className="ct-composer-attachment-capsule" data-testid="composer-plan-attachment" title={attachment.name} aria-label={`Attachment: ${attachment.name}`}>
            <span className="sr-only">Attachment: </span>{attachment.name}
          </span>
        ))}
        <button type="button" className="ct-composer-activity-chip__expand" data-testid="composer-plan-expand" aria-expanded={expanded} aria-controls={detailsId} aria-label={expanded ? "Collapse full plan reminder" : "Expand full plan reminder"} onMouseDown={(e) => e.preventDefault()} onClick={() => setExpanded((value) => !value)}>
          <ChevronDown size={14} aria-hidden />
        </button>
      </div>
      {expanded && <div id={detailsId} className="ct-composer-activity-details" data-testid="composer-plan-details" role="region" aria-label="Full plan reminder">
        <div className="whitespace-pre-wrap">{fullText}</div>
        {attachments.length > 0 && <div className="ct-composer-activity-details__attachments">{attachments.map((attachment) => <span key={attachment.id} className="ct-composer-attachment-capsule" title={attachment.name} aria-label={`Attachment: ${attachment.name}`}>{attachment.name}</span>)}</div>}
      </div>}
    </div>
  );
}
