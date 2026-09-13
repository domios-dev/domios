import { useMemo } from "react";
import { useMuxSelector } from "../../store/multiplexer";
import { tabsRecordEqual } from "../../store/multiplexer/selectors";
import { useFullPlans, usePlanIndexOptional } from "../Canvas/PlansProvider";
import { activeWorkForTab, selectActiveWork } from "../Canvas/planView/activeWork";
import { resolveTabComposerActivity } from "../Canvas/tabComposerActivity";
import { findPlanIndexForTab } from "../Canvas/planTabMatch";
import { usePlanTargetNavigation } from "../Canvas/teamViewNavigation";
import type { Tab } from "../../lib/types";
import { ComposerPlanActivityStrip } from "./ComposerPlanActivityStrip";

// compact human-title strip — orchestrating vs working — with no raw ids in UI.
export function TabTaskStrip({ tabId }: { tabId: string }) {
  const tabs = useMuxSelector((s) => s.tabs, tabsRecordEqual);
  const { index } = usePlanIndexOptional();
  const entries = useMemo(
    () => findPlanIndexForTab(index, tabId, tabs),
    [index, tabId, tabs],
  );
  if (entries.length === 0) return null;
  return <ResolvedTabTaskStrip tabId={tabId} tabs={tabs} />;
}

function ResolvedTabTaskStrip({
  tabId,
  tabs,
}: {
  tabId: string;
  tabs: Record<string, Tab>;
}) {
  const full = useFullPlans();
  const openPlanTarget = usePlanTargetNavigation();
  const activity = useMemo(
    () => resolveTabComposerActivity(full.plans, tabId, tabs),
    [full.plans, tabId, tabs],
  );
  const tabActiveWork = useMemo(() => {
    const snapshot = selectActiveWork(full.plans, tabs);
    return activeWorkForTab(snapshot, tabId);
  }, [full.plans, tabId, tabs]);
  if (!activity) return null;
  const renderedTask = activity.kind === "working"
    ? full.plans.flatMap((plan) => plan.tasks).find((task) => task.id === activity.taskId) ?? tabActiveWork?.task
    : null;

  const onOpen = () => {
    openPlanTarget(activity.kind === "working"
      ? { planId: activity.planId, taskId: activity.taskId }
      : { planId: activity.planId });
  };

  return (
    <div className="ct-composer-activity-row mb-2 w-full min-w-0 max-w-full">
      <ComposerPlanActivityStrip
        activity={activity}
        onOpen={onOpen}
        taskStatus={activity.kind === "working" ? activity.taskStatus : null}
        stale={tabActiveWork?.stale ?? false}
        nextAction={activity.kind === "working" && activity.taskStatus === "blocked" ? "Resolve blocker" : activity.kind === "working" && activity.taskStatus === "in_review" ? "Review task" : activity.kind === "working" ? "Continue task" : activity.kind === "planning" ? "Shape the plan" : "Coordinate open work"}
        attachments={renderedTask?.attachments}
        detailText={renderedTask?.body}
      />
    </div>
  );
}
