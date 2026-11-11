import { invoke } from "./bridge";
import type { Tab, Workspace } from "./types";

interface TabMap { workspaces: readonly Workspace[]; tabs: Record<string, Tab> }

export function workspaceOfTab(state: TabMap, tabId: string): string | null {
  return state.workspaces.find(workspace => workspace.tabs.includes(tabId))?.id ?? null;
}

export function generalAgentTab(state: TabMap): { tabId: string; workspaceId: string } | null {
  for (const workspace of state.workspaces) {
    if (!workspace.protected) continue;
    const tabId = workspace.tabs.find(id => state.tabs[id]?.protected);
    if (tabId) return { tabId, workspaceId: workspace.id };
  }
  return null;
}

export function focusTab(tabId: string, workspaceId: string | null) {
  window.dispatchEvent(new CustomEvent("codeterm:focus-tab", { detail: { tabId, ...(workspaceId ? { workspaceId } : {}) } }));
}

export async function askGeneralAgent(state: TabMap, text: string): Promise<void> {
  const general = generalAgentTab(state);
  if (!general) throw new Error("The General Agent is not running. Open General and try again.");
  await invoke("chat_send_text", { tabId: general.tabId, text });
  focusTab(general.tabId, general.workspaceId);
}
