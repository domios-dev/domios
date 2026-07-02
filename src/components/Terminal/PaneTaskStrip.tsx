import { useMemo } from "react";
import { useMultiplexer } from "../../store/multiplexer";
import { useFullPlans, usePlanIndexOptional } from "../Canvas/PlansProvider";
import { resolvePaneComposerActivity } from "../Canvas/paneComposerActivity";
import { findPlanIndexForPane } from "../Canvas/planPaneMatch";
import { openPanePlanFocus } from "../Canvas/workspacePlanEntry";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Pane } from "../../lib/types";
import { ComposerPlanActivityStrip } from "./ComposerPlanActivityStrip";

// Above-composer context: index gate first (most panes skip full bodies), then a
// compact human-title strip — orchestrating vs working — with no raw ids in UI.
export function PaneTaskStrip({ paneId }: { paneId: string }) {
  const { state } = useMultiplexer();
  const { index } = usePlanIndexOptional();
  const entries = useMemo(
    () => findPlanIndexForPane(index, paneId, state.panes),
    [index, paneId, state.panes],
  );
  if (entries.length === 0) return null;
  return <ResolvedPaneTaskStrip paneId={paneId} entries={entries} panes={state.panes} />;
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
  if (!activity) return null;

  const onOpen = () => {
    if (activity.kind === "working") {
      openPanePlanFocus(entries, paneId, [activity.taskId]);
      return;
    }
    openPanePlanFocus(entries, paneId);
  };

  return <ComposerPlanActivityStrip activity={activity} onOpen={onOpen} />;
}
