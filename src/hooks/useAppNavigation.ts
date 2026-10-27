import { useEffect, useRef } from "react";
import { HYDRATED_GENERATION } from "../store/multiplexer/initialState";
import { optionalNavigationId, parentWhenMissing } from "../lib/viewerNavigation";
import { useViewerNavigation } from "./useViewerNavigation";

const VIEWS = ["workspace", "settings", "memory", "mesh", "neighbors", "extensions", "analytics"] as const;
export type MainView = typeof VIEWS[number];
const mainView = (value: unknown): MainView | undefined => VIEWS.find((entry) => entry === value);
const jobsScope = (value: unknown) => value === "machine" || value === "workspace" ? value : undefined;

export function useAppNavigation(generation: number, focusedTabId: string | null, workspaces: readonly { id: string }[]) {
  const [view, setMainView] = useViewerNavigation<MainView>("view", "workspace", mainView);
  const [automationWorkspaceId, setAutomationWorkspaceId] = useViewerNavigation<string | null>("jobs-workspace", null, optionalNavigationId);
  const [scope, setJobsScope] = useViewerNavigation<"machine" | "workspace">("jobs-scope", "workspace", jobsScope);
  const [workspaceMapWorkspaceId, setWorkspaceMapWorkspaceId] = useViewerNavigation<string | null>("workspace-map", null, optionalNavigationId);
  const lastFocus = useRef({ ready: generation > HYDRATED_GENERATION, tabId: focusedTabId });
  useEffect(() => {
    const ready = generation > HYDRATED_GENERATION;
    if (ready && lastFocus.current.ready && focusedTabId !== lastFocus.current.tabId && focusedTabId !== null) setMainView("workspace");
    lastFocus.current = { ready, tabId: focusedTabId };
  }, [generation, focusedTabId, setMainView]);
  useEffect(() => {
    const exists = (id: string) => workspaces.some((workspace) => workspace.id === id);
    const ready = generation > HYDRATED_GENERATION;
    setAutomationWorkspaceId((id) => parentWhenMissing(id, ready, exists));
    setWorkspaceMapWorkspaceId((id) => parentWhenMissing(id, ready, exists));
  }, [generation, workspaces, automationWorkspaceId, workspaceMapWorkspaceId, setAutomationWorkspaceId, setWorkspaceMapWorkspaceId]);
  return { navigationReady: generation > HYDRATED_GENERATION, mainView: view, setMainView, automationWorkspaceId, setAutomationWorkspaceId, jobsScope: scope, setJobsScope, workspaceMapWorkspaceId, setWorkspaceMapWorkspaceId };
}
