import { Workflow } from "lucide-react";
import { presenceColor } from "../Canvas/presence/identity";
import type { PaneTabActivity } from "../Canvas/paneTabActivity";
import type { PaneStatus } from "../../lib/types";

// EMERGENCE (north-star #2): the per-tab swarm glance. Deliberately TINY — a single
// 7px token that never widens or clips the tab title (shrink-0, leading-none). A
// WORKER wears its board-identity dot (same presenceColor as its agent circle on the
// canvas); an ORCHESTRATOR wears the plan glyph (same hue as its progress strip). The
// token pulses while its pane is executing — liveness without a second moving part.
const ORCH_ACCENT = "rgba(82,180,145,0.95)"; // matches OrchestratorProgressStrip

export function TabActivityIndicator({
  activity,
  paneId,
  status,
}: {
  activity: PaneTabActivity;
  paneId: string;
  status?: PaneStatus | null;
}) {
  const live = status === "Working";
  const more = activity.planCount > 1 ? ` · +${activity.planCount - 1} more` : "";

  if (activity.kind === "orchestrator") {
    const title = `${activity.planTitle} · ${activity.taskCount} task${activity.taskCount === 1 ? "" : "s"}${more}\nDrives this plan`;
    return (
      <span
        data-testid="tab-activity"
        data-activity-kind="orchestrator"
        data-plan-id={activity.planId}
        title={title}
        className={`shrink-0 inline-flex items-center leading-none ${live ? "ct-presence-live rounded-full" : ""}`}
        style={{ color: ORCH_ACCENT, ["--ct-presence-color" as string]: ORCH_ACCENT }}
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
      className={`shrink-0 block rounded-full ${live ? "ct-presence-live" : ""}`}
      style={{
        width: 7,
        height: 7,
        background: color,
        boxShadow: `0 0 0 1px color-mix(in srgb, ${color} 55%, transparent)`,
        ["--ct-presence-color" as string]: color,
      }}
    />
  );
}
