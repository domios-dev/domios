import type { ExtendedState } from "./tree";
import type { MultiplexerState, Tab, Workspace } from "../../lib/types";

export function snapWithTabs(
  generation: number,
  pairs: Array<[string, string]>,
): MultiplexerState {
  const tabs: Record<string, Tab> = {};
  for (const [, tabId] of pairs) tabs[tabId] = { id: tabId, title: tabId } as unknown as Tab;
  return {
    workspaces: [
      {
        id: "w1",
        name: "W",
        tabs: pairs.map(([, tabId]) => tabId),
        active_tab_id: pairs[0]?.[1] ?? null,
        cwd: null,
      } as unknown as Workspace,
    ],
    tabs,
    active_workspace_id: "w1",
    focused_tab_id: null,
    generation,
  };
}

export function tabIds(state: ExtendedState): string {
  return state.workspaces[0]?.tabs.join(",") ?? "";
}

export function snapWithProviderTab(
  generation: number,
  agentType: string | null,
  model: string | null,
): MultiplexerState {
  return {
    workspaces: [
      { id: "w1", name: "W", tabs: ["ga"], active_tab_id: "ga", cwd: null } as unknown as Workspace,
    ],
    tabs: {
      ga: {
        id: "ga",
        pty_id: "pty-1",
        title: "Main",
        agent_type: agentType,
        agent_session_model: model,
      } as unknown as Tab,
    },
    active_workspace_id: "w1",
    focused_tab_id: "ga",
    generation,
  };
}
