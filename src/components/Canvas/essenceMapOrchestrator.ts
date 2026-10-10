import { invoke } from "../../lib/bridge";
import { ORCHESTRATOR_KIND, roleKind } from "../../lib/roleKind";
import type { Tab } from "../../lib/types";

export interface MapOrchestrator {
  tabId: string;
  title: string;
}

/** The first live Orchestrator in workspace tab order; a dead one has lost its agent identity. */
export function findWorkspaceOrchestrator(
  workspace: { tabs: string[] },
  tabs: Record<string, Tab>,
): MapOrchestrator | null {
  for (const tabId of workspace.tabs) {
    const tab = tabs[tabId];
    if (tab && tab.agent_type != null && roleKind(tab) === ORCHESTRATOR_KIND) return { tabId: tab.id, title: tab.title };
  }
  return null;
}

export function buildProjectMapRequest(projectPaths: readonly string[]): string {
  const targets = projectPaths.length > 0 ? projectPaths.join(", ") : "this workspace";
  return `Build the project essence map (essences/map.jsonl) for ${targets}. The Map view has no source-backed nodes yet; use the Cartographer if that is the right way to do it.`;
}

export async function askOrchestratorToBuildMap(workspaceId: string, orchestrator: MapOrchestrator, projectPaths: readonly string[]): Promise<void> {
  await invoke("chat_send_text", { tabId: orchestrator.tabId, text: buildProjectMapRequest(projectPaths) });
  window.dispatchEvent(new CustomEvent("codeterm:focus-tab", { detail: { tabId: orchestrator.tabId, workspaceId } }));
}
