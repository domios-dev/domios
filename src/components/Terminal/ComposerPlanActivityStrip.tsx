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
      className="mb-2 flex w-full min-w-0 items-center gap-1.5 overflow-hidden rounded-md border px-2 py-1 text-left transition hover:brightness-110"
      style={{
        background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 4%, transparent)",
        borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 22%, transparent)",
        cursor: onOpen ? "pointer" : "default",
      }}
    >
      <span
        data-testid="composer-plan-activity-prefix"
        className="shrink-0 font-medium"
        style={{ fontSize: "0.72rem", color: "var(--ct-muted-fg, #8b949e)" }}
      >
        {prefix}
      </span>
      <span
        data-testid="composer-plan-activity-primary"
        className="min-w-0 truncate font-medium"
        style={{ fontSize: "0.74rem", color: "var(--ct-fg, #c9d1d9)" }}
      >
        {primary}
      </span>
      {secondary && (
        <>
          <span aria-hidden className="shrink-0" style={{ fontSize: "0.72rem", color: "var(--ct-muted-fg, #8b949e)" }}>·</span>
          <span
            data-testid="composer-plan-activity-secondary"
            className="min-w-0 truncate"
            style={{ fontSize: "0.72rem", color: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 68%, transparent)" }}
          >
            {secondary}
          </span>
        </>
      )}
      {extraPlans > 0 && (
        <span
          data-testid="composer-plan-activity-extra-plans"
          className="shrink-0"
          style={{ fontSize: "0.68rem", fontWeight: 600, color: "var(--ct-muted-fg, #8b949e)" }}
        >
          +{extraPlans}
        </span>
      )}
      {onOpen && (
        <Map size={11} aria-hidden className="ml-auto shrink-0 opacity-45" style={{ color: "var(--ct-muted-fg, #8b949e)" }} />
      )}
    </button>
  );
}
