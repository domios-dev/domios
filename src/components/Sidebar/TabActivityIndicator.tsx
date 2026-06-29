import { Workflow } from "lucide-react";
import { presenceColor } from "../Canvas/presence/identity";
import type { PaneTabActivity } from "../Canvas/paneTabActivity";

// EMERGENCE (north-star #2): the per-tab swarm glance. Deliberately TINY — a single
// 7px token that never widens or clips the tab title (shrink-0, leading-none). A
// WORKER wears its board-identity dot (same presenceColor as its agent circle on the
// canvas); an ORCHESTRATOR wears the plan glyph (same hue as its plan presence).
const ORCH_ACCENT = "rgba(82,180,145,0.95)"; // matches OrchestratorProgressStrip

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
        data-plan-id={activity.planId}
        title={title}
        className="shrink-0 inline-flex items-center leading-none"
        style={{ color: ORCH_ACCENT }}
      >
        <Workflow size={11} strokeWidth={2.4} aria-hidden />
      </span>
    );
  }

  const color = presenceColor(paneId);
  const title = `${activity.planTitle}${more}\nExecuting a task in this plan`;
  return (
    <span
      data-testid="tab-activity"
      data-activity-kind="worker"
      data-plan-id={activity.planId}
      title={title}
      className="shrink-0 block rounded-full"
      style={{
        width: 7,
        height: 7,
        background: color,
        boxShadow: `0 0 0 1px color-mix(in srgb, ${color} 55%, transparent)`,
      }}
    />
  );
}
