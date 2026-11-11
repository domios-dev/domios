import { describe, expect, it } from "vitest";
import { orchestrationPairParams } from "./orchestrationPair";
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
});
