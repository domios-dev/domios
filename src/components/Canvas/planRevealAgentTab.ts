import type { Workspace } from "../../lib/types";
import { canvasFocusStore } from "./canvasFocusStore";

export interface TabContext {
  workspaceId: string;
  tabId: string;
}

export function findTabContext(
  workspaces: readonly Workspace[],
  tabId: string,
): TabContext | null {
  const target = tabId.trim();
  if (!target) return null;
  for (const ws of workspaces) {
    if (ws.tabs.includes(target)) return { workspaceId: ws.id, tabId: target };
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

  const ctx = findTabContext(deps.workspaces, target);
  if (ctx) deps.setActiveTab(ctx.workspaceId, ctx.tabId);
  deps.setFocusedTab(target);
  return true;
}
