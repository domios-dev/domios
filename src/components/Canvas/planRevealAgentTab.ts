import type { Workspace } from "../../lib/types";
import { canvasFocusStore } from "./canvasFocusStore";

export interface TabTabContext {
  workspaceId: string;
  tabId: string;
}

export function findTabTabContext(
  workspaces: readonly Workspace[],
  tabId: string,
): TabTabContext | null {
  const target = tabId.trim();
  if (!target) return null;
  for (const ws of workspaces) {
    for (const tab of ws.tabs) {
      if (tab.tab_id === target) {
        return { workspaceId: ws.id, tabId: tab.id };
      }
    }
  }
  return null;
}

export interface RevealAgentTabDeps {
  workspaces: readonly Workspace[];
  setActiveTab: (workspaceId: string, tabId: string) => void;
  setFocusedTab: (tabId: string) => void;
}

export function revealAssignedAgentTab(deps: RevealAgentTabDeps, tabId: string): boolean {
  const target = tabId.trim();
  if (!target) return false;

  canvasFocusStore.close();
  canvasFocusStore.closeWorkspace();

  const ctx = findTabTabContext(deps.workspaces, target);
  if (ctx) deps.setActiveTab(ctx.workspaceId, ctx.tabId);
  deps.setFocusedTab(target);
  return true;
}
