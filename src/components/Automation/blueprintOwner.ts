import { ALL_JOBS_SCOPE, type BlueprintSnapshot } from "./types";
import { hostAutomationAdapter } from "./hostAdapter";
import { createOrchestrationTransport } from "../../lib/transport/orchestration";
import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import { invoke } from "../../lib/bridge";
import type { RemotePeerTabs } from "../../lib/remoteTabPortal";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { runningBlueprintNamesForOwner } from "./blueprintOwnerModel";

export async function readWorkspaceBlueprints(workspaceId: string): Promise<readonly BlueprintSnapshot[]> {
  const graph = await hostAutomationAdapter.read({ connection_id: "owner-close", workspace_id: workspaceId, team_id: ALL_JOBS_SCOPE }, new AbortController().signal);
  return graph.blueprints ?? [];
}

export async function readTabCloseBlueprints(workspaceId: string, tabId: string, ownerWorkspaceId?: string): Promise<string[]> {
  const remote = parseCompositeTabId(tabId);
  if (!remote) {
    return runningBlueprintNamesForOwner(await readWorkspaceBlueprints(workspaceId), "tab", tabId);
  }
  const ownerId = ownerWorkspaceId ?? (await invoke<RemotePeerTabs>("mesh_peer_tabs_list", { peerId: remote.peerId }))
    .tabs.find(tab => tab.id === remote.remoteTabId)?.workspace_id;
  if (!ownerId) throw new Error("The owning machine did not list this tab's workspace.");
  const graph = await createOrchestrationTransport(daemonTransportForPeer(remote.peerId))
    .automation_graph_workspace_snapshot({ workspaceId: ownerId });
  return runningBlueprintNamesForOwner(graph.blueprints ?? [], "tab", remote.remoteTabId);
}
