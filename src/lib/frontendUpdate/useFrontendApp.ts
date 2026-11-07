import { useEffect, useRef, useState } from "react";
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
  const latest = useRef(selection);
  latest.current = selection;
  const { generation, workspaceId, tabId } = selection;
  const pageScoped = !!selection.pageSnapshot;
  const savedSelection = useRef<{ workspaceId: string; tabId: string | null } | null>(null);
  useEffect(() => {
    const selection = latest.current;
    if (selection.generation <= 0) return;
    if (selection.pageSnapshot && selection.generation < 1) return;
    if (!restored) {
      const checkpoint = frontendSignals.checkpointView();
      if (selection.pageSnapshot) {
        const saved = loadClientPageSelection();
        savedSelection.current = saved;
        const route = initialPageSelection(selection.pageSnapshot, saved);
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
    // Every client remembers its own selection so a reload restores where this client was.
    if (selection.workspaceId) {
      if (savedSelection.current?.workspaceId !== selection.workspaceId || savedSelection.current?.tabId !== selection.tabId) {
        savedSelection.current = { workspaceId: selection.workspaceId, tabId: selection.tabId };
        saveClientPageSelection(savedSelection.current);
      }
    }
    frontendSignals.editSelection(selection.workspaceId, selection.tabId);
    frontendRootRendered();
    frontendSignals.signal({ idle: true });
  }, [generation, workspaceId, tabId, pageScoped, restored]);
  return restored && selection.generation > 0;
}
