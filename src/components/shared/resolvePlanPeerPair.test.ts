// Run: npx tsx src/components/shared/resolvePlanPeerPair.test.ts

import type { Pane } from "../../lib/types";
import type { Plan } from "../Canvas/types";
import {
  findOwnedPlanForOrchestrator,
  resolvePlanPeerPair,
  resolvePlannerPaneIdFromPlan,
} from "./resolvePlanPeerPair";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkPane(id: string, over: Partial<Pane> = {}): Pane {
  return {
    id,
    title: id,
    cwd: null,
    workspace_id: "ws-1",
    hidden: false,
    ...over,
  } as Pane;
}

function mkPlan(over: Partial<Plan> = {}): Plan {
  return {
    id: "pl-1",
    title: "Plan",
    status: "active",
    mode: "execution",
    owner: { pane_id: "orch-1", session_id: null, title: "Orch" },
    planner: { kind: "pane", pane_id: "plan-1", session_id: null, title: "Planner" },
    tasks: [],
    sections: [],
    updated_at: 100,
    created_at: 1,
    ...over,
  } as Plan;
}

{
  const plan = mkPlan({
    planner: { kind: "pane", pane_id: "peer-planner", session_id: null, title: null },
  });
  assert(resolvePlannerPaneIdFromPlan(plan) === "peer-planner", "planner Tab id comes from planner ref");
}

{
  const plan = mkPlan({ planner: { kind: "waived", note: "solo" } });
  assert(resolvePlannerPaneIdFromPlan(plan) === null, "waived → no Tab");
}

{
  const plans = [mkPlan()];
  const panes = {
    "orch-1": mkPane("orch-1", { role: "Orchestrator" }),
    "plan-1": mkPane("plan-1", { role: "Manager", role_profile: "planner", parent_pane_id: "orch-1" }),
    "worker-1": mkPane("worker-1", { role: "Worker", parent_pane_id: "orch-1" }),
  };
  const pair = resolvePlanPeerPair("orch-1", plans, panes);
  assert(pair?.orchestratorPaneId === "orch-1", "orchestrator id");
  assert(pair?.plannerPaneId === "plan-1", "planner from ref");
  assert(pair?.planId === "pl-1", "plan id");
}

{
  const plans = [
    mkPlan({
      owner: { pane_id: "orch-other", session_id: null, title: null },
      planner: { kind: "pane", pane_id: "plan-1", session_id: null, title: null },
    }),
  ];
  const panes = {
    "orch-1": mkPane("orch-1", { role: "Orchestrator" }),
    "plan-1": mkPane("plan-1", { parent_pane_id: "orch-1" }),
  };
  assert(findOwnedPlanForOrchestrator("orch-1", plans, panes) === null, "no owner match");
}

console.log("✓ resolvePlanPeerPair: planner_ref semantics");
