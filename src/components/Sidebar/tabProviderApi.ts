import { invoke } from "../../lib/bridge";
import type { TabProviderSwitchApi } from "./tabProviderMenu";

const defaultApi: TabProviderSwitchApi = {
  restartWithProvider: (tabId, providerId) =>
    invoke("restart_agent_tab", { tabId, providerId }),
};

let apiOverride: TabProviderSwitchApi | null = null;

export function getTabProviderApi(): TabProviderSwitchApi {
  return apiOverride ?? defaultApi;
}

export function __setTabProviderApiForTest(api: TabProviderSwitchApi | null): void {
  apiOverride = api;
}
