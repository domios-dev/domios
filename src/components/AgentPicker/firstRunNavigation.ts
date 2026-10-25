import { isGeneralAgent } from "../../lib/roleKind";
import type { MultiplexerState } from "../../lib/types";
import type { TabRef } from "./pickerNavigation";

export function firstRunGeneralTarget(state: Pick<MultiplexerState, "tabs" | "workspaces">): TabRef | null {
  for (const workspace of state.workspaces) {
    const tabId = workspace.tabs.find((id) => {
      const tab = state.tabs[id];
      return tab?.protected && isGeneralAgent(tab) && !tab.hidden && tab.show_in_tabbar !== false;
    });
    if (tabId) return { workspaceId: workspace.id, tabId };
  }
  return null;
}
