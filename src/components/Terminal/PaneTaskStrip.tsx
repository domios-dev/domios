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

// Above-composer container: a cheap index gate decides whether THIS pane has an
// assigned task at all (most panes don't), and only then do we pull full plan
// bodies to resolve which task + who is on it. No task → renders nothing.
export function PaneTaskStrip({ paneId }: { paneId: string }) {
  const { state } = useMultiplexer();
  const { index } = usePlanIndexOptional();
  const entries = useMemo(
    () => findPlanIndexForPane(index, paneId, state.panes),
    [index, paneId, state.panes],
  );
  const assigned = useMemo(
    () => index.some((e) => e.status === "active" && e.assigned_pane_ids.includes(paneId)),
    [index, paneId],
  );
  if (!assigned || entries.length === 0) return null;
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
  const agent = useMemo(() => {
    if (!resolved) return null;
    return buildTaskPresence(resolved.plan, resolved.task, panes)?.agents[0] ?? null;
  }, [resolved, panes]);
  if (!resolved) return null;
  return (
    <WorkerTaskStrip
      task={resolved.task}
      branch={resolved.branch}
      agent={agent}
      onOpen={() => openPanePlanFocus(entries, paneId, [resolved.task.id])}
    />
  );
}
