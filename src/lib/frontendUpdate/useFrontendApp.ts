import { useEffect, useState } from "react";
import { frontendRootRendered } from "./browser";
import { frontendSignals } from "./appState";

export interface FrontendSelection {
  generation: number;
  workspaceId: string | null;
  tabId: string | null;
  hasWorkspace(id: string): boolean;
  hasTab(id: string): boolean;
  selectWorkspace(id: string): void;
  selectTab(workspaceId: string, tabId: string): void;
}

export function useFrontendApp(selection: FrontendSelection): void {
  const [restored, setRestored] = useState(false);
  useEffect(() => {
    if (selection.generation <= 0) return;
    if (!restored) {
      const checkpoint = frontendSignals.checkpoint();
      if (checkpoint.activeWorkspaceId && selection.hasWorkspace(checkpoint.activeWorkspaceId)) {
        selection.selectWorkspace(checkpoint.activeWorkspaceId);
        if (checkpoint.activeTabId && selection.hasTab(checkpoint.activeTabId)) selection.selectTab(checkpoint.activeWorkspaceId, checkpoint.activeTabId);
      }
      setRestored(true);
      return;
    }
    const checkpoint = frontendSignals.checkpoint();
    if (checkpoint.activeWorkspaceId !== selection.workspaceId || checkpoint.activeTabId !== selection.tabId) {
      checkpoint.activeWorkspaceId = selection.workspaceId;
      checkpoint.activeTabId = selection.tabId;
      frontendSignals.edit(checkpoint);
    }
    frontendRootRendered();
    frontendSignals.signal({ idle: true });
  }, [selection, restored]);
}
