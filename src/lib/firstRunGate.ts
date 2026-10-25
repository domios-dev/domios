import type { GeneralAgentConfig, Tab, Workspace } from "./types";
import type { PageSelection } from "../store/multiplexer/pageSelection";
import { isFirstRunGeneralTab } from "../components/AgentPicker/constants";

export const FIRST_RUN_DISABLED_REASON = "Choose your General agent first";

export function firstRunLocked(general: Pick<GeneralAgentConfig, "first_run_complete"> | undefined): boolean {
  return general?.first_run_complete !== true;
}

export function firstRunSelection<T extends PageSelection & { workspaces: readonly Workspace[]; tabs: Record<string, Tab> }>(locked: boolean, state: T): T {
  if (!locked) return state;
  const workspace = state.workspaces.find((ws) => ws.tabs.some((id) => isFirstRunGeneralTab(state.tabs[id])));
  const tabId = workspace?.tabs.find((id) => isFirstRunGeneralTab(state.tabs[id])) ?? null;
  return { ...state, active_workspace_id: workspace?.id ?? null, focused_tab_id: tabId,
    activeTabIds: workspace && tabId ? { ...state.activeTabIds, [workspace.id]: tabId } : state.activeTabIds };
}

export function firstRunSidebarTabs(locked: boolean, tabs: Record<string, Tab>): Record<string, Tab> {
  return locked ? Object.fromEntries(Object.entries(tabs).filter(([, tab]) => isFirstRunGeneralTab(tab))) : tabs;
}
