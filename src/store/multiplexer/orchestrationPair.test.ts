import assert from "node:assert/strict";
import {requestOrchestrationPair} from "./orchestrationPair";

async function main() {
  const calls: {command: string; args?: Record<string, unknown>}[] = [];
  await requestOrchestrationPair(async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({command, args});
    return {orchestrator_tab_id: "lead", planner_tab_id: "planner"} as T;
  }, {
    orchestratorAgent: "runtime", orchestratorLaunchProfile: "alpha", orchestratorModel: "large", orchestratorReasoningEffort: "high",
    plannerAgent: "runtime", plannerLaunchProfile: "beta", plannerModel: "small", plannerReasoningEffort: "low", workspaceId: "workspace",
  });
  assert.deepEqual(calls, [{command: "mux_create_orchestration_pair", args: {params: {
    orchestrator_agent: "runtime", orchestrator_launch_profile: "alpha", orchestrator_model: "large", orchestrator_reasoning_effort: "high",
    planner_agent: "runtime", planner_launch_profile: "beta", planner_model: "small", planner_reasoning_effort: "low", workspace_id: "workspace",
    title: undefined, existing_tab_id: undefined,
  }}}]);
  console.log("1/1 passed: pair request preserves independent role launch options");
}

void main();
