import { Map } from "lucide-react";
import type { PaneComposerActivity } from "../Canvas/paneComposerActivity";

export interface ComposerPlanActivityStripProps {
  activity: PaneComposerActivity;
  onOpen?: () => void;
}

function activityLabel(activity: PaneComposerActivity): { prefix: string; primary: string; secondary?: string } {
  if (activity.kind === "orchestrating") {
    return { prefix: "Orchestrating:", primary: activity.planTitle };
  }
  return {
    prefix: "Working on:",
    primary: activity.taskTitle,
    secondary: activity.planTitle,
  };
}

export function ComposerPlanActivityStrip({ activity, onOpen }: ComposerPlanActivityStripProps) {
  const { prefix, primary, secondary } = activityLabel(activity);
  const extraPlans = activity.kind === "orchestrating" ? activity.extraPlans : 0;
  const title = activity.kind === "orchestrating"
    ? `${prefix} ${primary}${extraPlans > 0 ? ` (+${extraPlans} more plan${extraPlans === 1 ? "" : "s"})` : ""}\nOpen plan on canvas`
    : `${prefix} ${primary}${secondary ? ` · ${secondary}` : ""}\nOpen task on canvas`;

  return (
    <button
      type="button"
      data-testid="composer-plan-activity-strip"
      data-activity-kind={activity.kind}
      data-plan-id={activity.planId}
      data-task-id={activity.kind === "working" ? activity.taskId : undefined}
      title={title}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onOpen}
      className="ct-composer-activity-chip h-7 max-w-full min-w-0 hover:brightness-110"
      style={{ cursor: onOpen ? "pointer" : "default" }}
    >
      <span data-testid="composer-plan-activity-prefix" className="ct-composer-activity-chip__prefix">
        {prefix}
      </span>
      <span data-testid="composer-plan-activity-primary" className="ct-composer-activity-chip__primary">
        {primary}
      </span>
      {secondary && (
        <>
          <span aria-hidden className="ct-composer-activity-chip__sep">·</span>
          <span data-testid="composer-plan-activity-secondary" className="ct-composer-activity-chip__secondary">
            {secondary}
          </span>
        </>
      )}
      {extraPlans > 0 && (
        <span data-testid="composer-plan-activity-extra-plans" className="ct-composer-activity-chip__extra">
          +{extraPlans}
        </span>
      )}
      {onOpen && (
        <Map size={11} aria-hidden className="ct-composer-activity-chip__icon" />
      )}
    </button>
  );
}
