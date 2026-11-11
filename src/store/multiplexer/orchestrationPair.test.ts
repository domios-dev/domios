import { describe, expect, it } from "vitest";
import { orchestrationPairParams, requestOrchestrationPair } from "./orchestrationPair";
import { templateStartRequestFixture } from "../../lib/transport/teamTemplates.testdata";

describe("orchestration pair params", () => {
  const pair = { orchestratorAgent: "claude", plannerAgent: "codex", workspaceId: "workspace" };

  it("omits template for the default pair, exactly as before templates", () => {
    expect(JSON.parse(JSON.stringify(orchestrationPairParams(pair)))).toEqual({
      orchestrator_agent: "claude", planner_agent: "codex", workspace_id: "workspace",
    });
  });

  it("carries a chosen template on the same pair request", () => {
    expect(orchestrationPairParams({ ...pair, template: templateStartRequestFixture }).template).toEqual(templateStartRequestFixture);
  });

  it("preserves independent role launch options", async () => {
    const calls: { command: string; args?: Record<string, unknown> }[] = [];
    await requestOrchestrationPair(async <T>(command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      return { orchestrator_tab_id: "lead", planner_tab_id: "planner" } as T;
    }, {
      orchestratorAgent: "runtime", orchestratorLaunchProfile: "alpha", orchestratorModel: "large", orchestratorReasoningEffort: "high",
      plannerAgent: "runtime", plannerLaunchProfile: "beta", plannerModel: "small", plannerReasoningEffort: "low", workspaceId: "workspace",
    });
    expect(calls).toEqual([{ command: "mux_create_orchestration_pair", args: { params: {
      orchestrator_agent: "runtime", orchestrator_launch_profile: "alpha", orchestrator_model: "large", orchestrator_reasoning_effort: "high",
      planner_agent: "runtime", planner_launch_profile: "beta", planner_model: "small", planner_reasoning_effort: "low", workspace_id: "workspace",
    } } }]);
  });
});
