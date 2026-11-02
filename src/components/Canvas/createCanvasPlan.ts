import type { ApiConfig } from "../../lib/types";
import type { CanvasWorkspaceTarget } from "./canvasFocusStore";
import type { CreatePlanParams } from "./plansApi";
interface CreateCanvasPlanArgs {
  api: ApiConfig;
  projectPath?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  createPlan: (api: ApiConfig, params: CreatePlanParams) => Promise<unknown>;
}

interface CreatedPlanOwnership {
  plan?: {
    owner?: { tab_id?: string | null };
    team_id?: string | null;
  };
  team_id?: string | null;
}

function verifyCreatedOwnership(created: unknown, tabId: string, expectedTeamId?: string): void {
  const result = created as CreatedPlanOwnership;
  if (result.plan?.owner?.tab_id !== tabId) {
    throw new Error("Created plan owner does not match the Canvas orchestrator");
  }
  if (!result.team_id || result.plan?.team_id !== result.team_id) {
    throw new Error("Created plan Team binding does not match the minted Team");
  }
  if (expectedTeamId && result.team_id !== expectedTeamId) {
    throw new Error("Created plan Team binding does not match the selected Team");
  }
}

export async function createCanvasPlan({
  api,
  projectPath,
  workspaceTarget,
  createPlan,
}: CreateCanvasPlanArgs): Promise<void> {
  const teamId = workspaceTarget?.teamId;
  const tabId = workspaceTarget?.managerTabId;
  if (!teamId || !tabId) throw new Error("Plan creation requires a Team manager context");
  const created = await createPlan(api, {
    title: "New plan",
    project_path: projectPath ?? undefined,
    owner: tabId
      ? {
        tab_id: tabId,
        session_id: null,
        title: "Team Manager",
      }
      : undefined,
  });
  verifyCreatedOwnership(created, tabId, teamId);
}

