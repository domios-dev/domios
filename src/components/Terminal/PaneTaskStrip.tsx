import { useMemo } from "react";
import { useMuxSelector } from "../../store/multiplexer";
import { panesRecordEqual } from "../../store/multiplexer/selectors";
import { useFullPlans, usePlanIndexOptional } from "../Canvas/PlansProvider";
import { activeWorkForPane, selectActiveWork } from "../Canvas/planView/activeWork";
import { resolvePaneComposerActivity } from "../Canvas/paneComposerActivity";
import { findPlanIndexForPane } from "../Canvas/planPaneMatch";
import { openPanePlanFocus } from "../Canvas/workspacePlanEntry";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Pane } from "../../lib/types";
import { ComposerPlanActivityStrip } from "./ComposerPlanActivityStrip";

// compact human-title strip — orchestrating vs working — with no raw ids in UI.
export function PaneTaskStrip({ paneId }: { paneId: string }) {
  const panes = useMuxSelector((s) => s.panes, panesRecordEqual);
  const { index } = usePlanIndexOptional();
  const entries = useMemo(
    () => findPlanIndexForPane(index, paneId, panes),
    [index, paneId, panes],
  );
  if (entries.length === 0) return null;
  return <ResolvedPaneTaskStrip paneId={paneId} entries={entries} panes={panes} />;
}

function ResolvedPaneTaskStrip({
  paneId,
  entries,
  panes,
}: {
  paneId: string;
  entries: PlanIndexEntry[];
  panes: Record<string, Pane>;
}) {
  const full = useFullPlans();
  const activity = useMemo(
    () => resolvePaneComposerActivity(full.plans, paneId, panes),
    [full.plans, paneId, panes],
  );
  const paneActiveWork = useMemo(() => {
    const snapshot = selectActiveWork(full.plans, panes);
    return activeWorkForPane(snapshot, paneId);
  }, [full.plans, paneId, panes]);
  if (!activity) return null;
  const renderedTask = activity.kind === "working"
    ? full.plans.flatMap((plan) => plan.tasks).find((task) => task.id === activity.taskId) ?? paneActiveWork?.task
    : null;

  const onOpen = () => {
    if (activity.kind === "working") {
      openPanePlanFocus(entries, paneId, [activity.taskId]);
      return;
    }
    openPanePlanFocus(entries, paneId);
  };

  return (
    <div className="ct-composer-activity-row mb-2 w-full min-w-0 max-w-full">
      <ComposerPlanActivityStrip
        activity={activity}
        onOpen={onOpen}
        taskStatus={activity.kind === "working" ? activity.taskStatus : null}
        stale={paneActiveWork?.stale ?? false}
        nextAction={activity.kind === "working" && activity.taskStatus === "blocked" ? "Resolve blocker" : activity.kind === "working" && activity.taskStatus === "in_review" ? "Review task" : activity.kind === "working" ? "Continue task" : activity.kind === "planning" ? "Shape the plan" : "Coordinate open work"}
        attachments={renderedTask?.attachments}
        detailText={renderedTask?.body}
      />
    </div>
  );
}
