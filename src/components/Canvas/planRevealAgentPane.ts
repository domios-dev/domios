import type { Workspace } from "../../lib/types";
import { canvasFocusStore } from "./canvasFocusStore";

export interface PaneTabContext {
  workspaceId: string;
  tabId: string;
}

export function findPaneTabContext(
  workspaces: readonly Workspace[],
  paneId: string,
): PaneTabContext | null {
  const target = paneId.trim();
  if (!target) return null;
  for (const ws of workspaces) {
    for (const tab of ws.tabs) {
      if (tab.pane_id === target) {
        return { workspaceId: ws.id, tabId: tab.id };
      }
    }
  }
  return null;
}

export interface RevealAgentPaneDeps {
  workspaces: readonly Workspace[];
  setActiveTab: (workspaceId: string, tabId: string) => void;
  setFocusedPane: (paneId: string) => void;
}

export function revealAssignedAgentPane(deps: RevealAgentPaneDeps, paneId: string): boolean {
  const target = paneId.trim();
  if (!target) return false;

  canvasFocusStore.close();
  canvasFocusStore.closeWorkspace();

  const ctx = findPaneTabContext(deps.workspaces, target);
  if (ctx) deps.setActiveTab(ctx.workspaceId, ctx.tabId);
  deps.setFocusedPane(target);
  return true;
}
