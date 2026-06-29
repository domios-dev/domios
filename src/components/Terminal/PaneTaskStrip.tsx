import { useMemo } from "react";
import { useMultiplexer } from "../../store/multiplexer";
import { useFullPlans, usePlanIndexOptional } from "../Canvas/PlansProvider";
import { findPlanIndexForPane } from "../Canvas/planPaneMatch";
import { resolvePaneCurrentTask } from "../Canvas/paneCurrentTask";
import { buildTaskPresence } from "../Canvas/taskPresence";
import { openPanePlanFocus } from "../Canvas/workspacePlanEntry";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Pane } from "../../lib/types";
import { WorkerTaskStrip } from "./WorkerTaskStrip";

// Above-composer container: a cheap index gate decides whether THIS pane relates to
// any active plan at all (most panes don't). Only then do we pull full plan bodies
// to show the concrete task this pane is executing. Plan ownership/progress stays
// on the canvas and workspace action row, not as a composer bubble.
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
  const resolved = useMemo(() => resolvePaneCurrentTask(full.plans, paneId), [full.plans, paneId]);
  const taskAgent = useMemo(() => {
    if (!resolved) return null;
    return buildTaskPresence(resolved.plan, resolved.task, panes)?.agents[0] ?? null;
  }, [resolved, panes]);

  if (!resolved) return null;
  return (
    <WorkerTaskStrip
      task={resolved.task}
      branch={resolved.branch}
      agent={taskAgent}
      onOpen={() => openPanePlanFocus(entries, paneId, [resolved.task.id])}
    />
  );
}
