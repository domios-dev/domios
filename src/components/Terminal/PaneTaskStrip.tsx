import { useMemo } from "react";
import { useMultiplexer } from "../../store/multiplexer";
import { useFullPlans, usePlanIndexOptional } from "../Canvas/PlansProvider";
import { findPlanIndexForPane } from "../Canvas/planPaneMatch";
import { resolvePaneCurrentTask } from "../Canvas/paneCurrentTask";
import { resolvePaneOwnedPlans } from "../Canvas/paneOwnedPlans";
import { buildTaskPresence } from "../Canvas/taskPresence";
import { openPanePlanFocus } from "../Canvas/workspacePlanEntry";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import type { Pane } from "../../lib/types";
import { WorkerTaskStrip } from "./WorkerTaskStrip";
import { OrchestratorProgressStrip } from "./OrchestratorProgressStrip";

// Above-composer container: a cheap index gate decides whether THIS pane relates to
// any active plan at all (most panes don't). Only then do we pull full plan bodies
// to choose the variant — an ORCHESTRATOR pane (owns a plan) wears plan progress; a
// WORKER pane wears its one task; a pane with neither renders nothing.
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
  const owned = useMemo(() => resolvePaneOwnedPlans(full.plans, paneId, panes), [full.plans, paneId, panes]);
  const resolved = useMemo(() => resolvePaneCurrentTask(full.plans, paneId), [full.plans, paneId]);
  const taskAgent = useMemo(() => {
    if (!resolved) return null;
    return buildTaskPresence(resolved.plan, resolved.task, panes)?.agents[0] ?? null;
  }, [resolved, panes]);

  if (owned.length > 0) {
    const top = owned[0];
    const planEntries = entries.filter((e) => e.plan_id === top.plan.id);
    return (
      <OrchestratorProgressStrip
        summary={top}
        extraPlans={owned.length - 1}
        onOpen={() => openPanePlanFocus(planEntries.length ? planEntries : entries, paneId)}
      />
    );
  }
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
