import type { Pane } from "../../lib/types";
import { ownerIsPaneSelf } from "./planPaneMatch";
import { resolvePlanExclusivePaneTasks } from "./paneCurrentTask";
import { buildTaskPresence } from "./taskPresence";
import type { PresenceAgent } from "./presence/identity";
import { TaskStatus, type Plan } from "./types";

export interface OrchestratorPlanSummary {
  plan: Plan;
  /** Tasks resolved (done). */
  done: number;
  /** Tasks that count toward progress — cancelled is out of scope, excluded. */
  total: number;
  pct: number;
  /** active_lanes from the plan heartbeat, falling back to in-progress task count. */
  activeLanes: number;
  /** Any task is executing → the strip pulses + the lead face rings live. */
  live: boolean;
  /** The orchestrator's current_focus narrative (heartbeat), null when absent. */
  focus: string | null;
  /** Distinct executors across the plan, leads before peers — the swarm face-stack. */
  agents: PresenceAgent[];
}

function summarize(plan: Plan, panes: Record<string, Pane>): OrchestratorPlanSummary {
  let done = 0;
  let total = 0;
  let live = false;
  const byAgent = new Map<string, PresenceAgent>();
  const exclusivePaneTasks = resolvePlanExclusivePaneTasks(plan);
  for (const task of plan.tasks) {
    if (task.status === TaskStatus.Cancelled) continue;
    total += 1;
    if (task.status === TaskStatus.Done) done += 1;
    if (task.status === TaskStatus.InProgress) live = true;
    const presence = buildTaskPresence(plan, task, panes, exclusivePaneTasks);
    for (const agent of presence?.agents ?? []) if (!byAgent.has(agent.id)) byAgent.set(agent.id, agent);
  }
  const hbLanes = (plan.heartbeat?.active_lanes ?? []).filter((l) => l.trim()).length;
  const inProgress = plan.tasks.filter((t) => t.status === TaskStatus.InProgress).length;
  const agents = [...byAgent.values()].sort((a, b) => Number(!!a.isPeer) - Number(!!b.isPeer));
  return {
    plan,
    done,
    total,
    pct: total > 0 ? Math.round((done / total) * 100) : 0,
    activeLanes: hbLanes > 0 ? hbLanes : inProgress,
    live,
    focus: plan.heartbeat?.current_focus?.trim() || null,
    agents,
  };
}

export function resolvePaneOwnedPlans(
  plans: readonly Plan[],
  paneId: string | null | undefined,
  panes: Record<string, Pane>,
): OrchestratorPlanSummary[] {
  if (!paneId) return [];
  const sessionId = panes[paneId]?.agent_session_id?.trim() || null;
  const owned = plans.filter(
    (p) => p.status === "active" && ownerIsPaneSelf(p.owner, paneId, sessionId),
  );
  const summaries = owned.map((p) => summarize(p, panes));
  summaries.sort(
    (a, b) =>
      Number(b.live) - Number(a.live) ||
      b.activeLanes - a.activeLanes ||
      b.plan.updated_at - a.plan.updated_at,
  );
  return summaries;
}
