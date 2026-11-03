import { useMemo } from "react";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { tabsRecordEqual } from "../../store/multiplexer/selectors";
import { usePlans } from "../Canvas/usePlans";
import { usePlansApiOptional, useSidebarPlansOptional, usePlanIndexOptional } from "../Canvas/PlansProvider";
import { activeWorkForTab, selectActiveWork } from "../Canvas/planView/activeWork";
import { resolveTabComposerActivity } from "../Canvas/tabComposerActivity";
import { findPlanIndexForTab } from "../Canvas/planTabMatch";
import { usePlanTargetNavigation } from "../Canvas/teamViewNavigation";

/** Cheap plan-index gate: true once any plan references the tab, before full plans load. */
export function useTabHasPlan(tabId: string): boolean {
  const tabs = useMuxSelector((s) => s.tabs, tabsRecordEqual);
  const { index } = usePlanIndexOptional();
  return useMemo(() => findPlanIndexForTab(index, tabId, tabs).length > 0, [index, tabId, tabs]);
}

export function useResolvedTabActivity(tabId: string, workspaceId: string | null, enabled = true) {
  const tabs = useMuxSelector((s) => s.tabs, tabsRecordEqual);
  const fallback = useSidebarPlansOptional();
  const api = usePlansApiOptional();
  const scoped = usePlans(api, { chat: true, tabId, enabled });
  const full = api ? scoped : fallback;
  const openPlanTarget = usePlanTargetNavigation();
  const activity = useMemo(
    () => resolveTabComposerActivity(full.plans, tabId, tabs),
    [full.plans, tabId, tabs],
  );
  const tabActiveWork = useMemo(() => {
    const snapshot = selectActiveWork(full.plans, tabs);
    return activeWorkForTab(snapshot, tabId);
  }, [full.plans, tabId, tabs]);
  const renderedTask = activity?.kind === "working"
    ? full.plans.flatMap((plan) => plan.tasks).find((task) => task.id === activity.taskId) ?? tabActiveWork?.task
    : null;
  const onOpen = () => {
    if (!activity) return;
    const target = activity.kind === "working"
      ? { planId: activity.planId, taskId: activity.taskId }
      : { planId: activity.planId };
    openPlanTarget({ ...target, returnTo: workspaceId ? { workspaceId, tabId } : undefined });
  };
  return { activity, onOpen, renderedTask, stale: tabActiveWork?.stale ?? false };
}
