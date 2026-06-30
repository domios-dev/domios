import type { PaneTabActivity } from "../Canvas/paneTabActivity";
import { presenceColor } from "../Canvas/presence/identity";

const ORCH_ACCENT = "rgba(82,180,145,0.72)";

function quietDot(color: string) {
  return {
    width: 5,
    height: 5,
    background: color,
  } as const;
}

export function TabActivityIndicator({
  activity,
  paneId,
}: {
  activity: PaneTabActivity;
  paneId: string;
}) {
  const more = activity.planCount > 1 ? ` · +${activity.planCount - 1} more` : "";

  if (activity.kind === "orchestrator") {
    const title = `${activity.planTitle} · ${activity.taskCount} task${activity.taskCount === 1 ? "" : "s"}${more}\nDrives this plan`;
    return (
      <span
        data-testid="tab-activity"
        data-activity-kind="orchestrator"
        data-tab-activity-quiet="true"
        data-plan-id={activity.planId}
        title={title}
        className="ct-tab-activity-quiet shrink-0 block rounded-full"
        style={quietDot(ORCH_ACCENT)}
      />
    );
  }

  const color = presenceColor(paneId);
  const title = `${activity.planTitle}${more}\nExecuting a task in this plan`;
  return (
    <span
      data-testid="tab-activity"
      data-activity-kind="worker"
      data-tab-activity-quiet="true"
      data-plan-id={activity.planId}
      title={title}
      className="ct-tab-activity-quiet shrink-0 block rounded-full"
      style={quietDot(color)}
    />
  );
}
