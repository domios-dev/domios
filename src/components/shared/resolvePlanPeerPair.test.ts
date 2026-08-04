// Run: npx tsx src/components/shared/resolvePlanPeerPair.test.ts

import type { Tab } from "../../lib/types";
import type { Plan } from "../Canvas/types";
import {
  findOwnedPlanForOrchestrator,
  resolvePlanPeerPair,
  resolvePlannerTabIdFromPlan,
} from "./resolvePlanPeerPair";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function mkTab(id: string, over: Partial<Tab> = {}): Tab {
  return {
    id,
    title: id,
    cwd: null,
    workspace_id: "ws-1",
    hidden: false,
    ...over,
  } as Tab;
}

function mkPlan(over: Partial<Plan> = {}): Plan {
  return {
    id: "pl-1",
    title: "Plan",
    status: "active",
    mode: "execution",
    owner: { tab_id: "orch-1", session_id: null, title: "Orch" },
    planner: { kind: "tab", tab_id: "plan-1", session_id: null, title: "Planner" },
    tasks: [],
    sections: [],
    updated_at: 100,
    created_at: 1,
    ...over,
  } as Plan;
}

{
  const plan = mkPlan({
    planner: { kind: "tab", tab_id: "peer-planner", session_id: null, title: null },
  });
  assert(resolvePlannerTabIdFromPlan(plan) === "peer-planner", "planner Tab id comes from planner ref");
}

{
  const plan = mkPlan({ planner: { kind: "waived", note: "solo" } });
  assert(resolvePlannerTabIdFromPlan(plan) === null, "waived → no Tab");
}

{
  const plans = [mkPlan()];
  const tabs = {
    "orch-1": mkTab("orch-1", { role: "Orchestrator" }),
    "plan-1": mkTab("plan-1", { role: "Manager", role_profile: "planner", parent_tab_id: "orch-1" }),
    "worker-1": mkTab("worker-1", { role: "Worker", parent_tab_id: "orch-1" }),
  };
  const pair = resolvePlanPeerPair("orch-1", plans, tabs);
  assert(pair?.orchestratorTabId === "orch-1", "orchestrator id");
  assert(pair?.plannerTabId === "plan-1", "planner from ref");
  assert(pair?.planId === "pl-1", "plan id");
}

{
  const plans = [
    mkPlan({
      owner: { tab_id: "orch-other", session_id: null, title: null },
      planner: { kind: "tab", tab_id: "plan-1", session_id: null, title: null },
    }),
  ];
  const tabs = {
    "orch-1": mkTab("orch-1", { role: "Orchestrator" }),
    "plan-1": mkTab("plan-1", { parent_tab_id: "orch-1" }),
  };
  assert(findOwnedPlanForOrchestrator("orch-1", plans, tabs) === null, "no owner match");
}

console.log("✓ resolvePlanPeerPair: planner_ref semantics");
