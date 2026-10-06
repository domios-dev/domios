import { ALL_JOBS_SCOPE, type BlueprintSnapshot } from "./types";
import { hostAutomationAdapter } from "./hostAdapter";

export async function readWorkspaceBlueprints(workspaceId: string): Promise<readonly BlueprintSnapshot[]> {
  const graph = await hostAutomationAdapter.read({ connection_id: "owner-close", workspace_id: workspaceId, team_id: ALL_JOBS_SCOPE }, new AbortController().signal);
  return graph.blueprints ?? [];
}
