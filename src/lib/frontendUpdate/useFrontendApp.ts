import { useEffect, useState } from "react";
import { frontendRootRendered } from "./browser";
import { frontendSignals } from "./appState";
import { initialPageSelection, type PageSelectionSnapshot } from "../../store/multiplexer/pageSelection";
import { loadClientPageSelection, saveClientPageSelection } from "../clientPageSelection";

export interface FrontendSelection {
  generation: number;
  workspaceId: string | null;
  tabId: string | null;
  hasWorkspace(id: string): boolean;
  hasTab(id: string): boolean;
  selectWorkspace(id: string): void;
  selectTab(workspaceId: string, tabId: string): void;
  pageSnapshot?: PageSelectionSnapshot;
}

export function useFrontendApp(selection: FrontendSelection): boolean {
  const [restored, setRestored] = useState(false);
  useEffect(() => {
    if (selection.generation <= 0) return;
    if (selection.pageSnapshot && selection.generation < 1) return;
    if (!restored) {
      const checkpoint = frontendSignals.checkpoint();
      if (selection.pageSnapshot) {
        const route = initialPageSelection(selection.pageSnapshot, loadClientPageSelection());
        if (route.active_workspace_id) {
          selection.selectWorkspace(route.active_workspace_id);
          const tabId = route.activeTabIds[route.active_workspace_id];
          if (tabId) selection.selectTab(route.active_workspace_id, tabId);
        }
      } else if (checkpoint.activeWorkspaceId && selection.hasWorkspace(checkpoint.activeWorkspaceId)) {
        selection.selectWorkspace(checkpoint.activeWorkspaceId);
        if (checkpoint.activeTabId && selection.hasTab(checkpoint.activeTabId)) selection.selectTab(checkpoint.activeWorkspaceId, checkpoint.activeTabId);
      }
      setRestored(true);
      return;
    }
    if (selection.pageSnapshot && selection.workspaceId) {
      saveClientPageSelection({ workspaceId: selection.workspaceId, tabId: selection.tabId });
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
  return restored && selection.generation > 0;
}
