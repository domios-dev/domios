/** Data seam for the sidebar row's provider switch. Unlike the model switch,
 *  this one relaunches the agent: pointing the tab at another provider is
 *  metadata until the session restarts under it. */
import { invoke } from "../../lib/bridge";
import { restartTabSession } from "../Terminal/tabSessionActions";
import type { TabProviderSwitchApi } from "./tabProviderMenu";

const defaultApi: TabProviderSwitchApi = {
  setAgentType: async (tabId, agentType) => {
    await invoke("mux_set_tab_agent_type", { tabId, agentType });
  },
  restartSession: (tabId) => restartTabSession(tabId),
};

let apiOverride: TabProviderSwitchApi | null = null;

export function getTabProviderApi(): TabProviderSwitchApi {
  return apiOverride ?? defaultApi;
}

export function __setTabProviderApiForTest(api: TabProviderSwitchApi | null): void {
  apiOverride = api;
}
