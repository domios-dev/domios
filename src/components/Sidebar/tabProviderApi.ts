import { invoke } from "../../lib/bridge";
import type { TabProviderSwitchApi } from "./tabProviderMenu";

const defaultApi: TabProviderSwitchApi = {
  switchProvider: (tabId, providerId, options) =>
    invoke("switch_agent_provider", { tabId, providerId, ...options }),
};

let apiOverride: TabProviderSwitchApi | null = null;

export function getTabProviderApi(): TabProviderSwitchApi {
  return apiOverride ?? defaultApi;
}

export function __setTabProviderApiForTest(api: TabProviderSwitchApi | null): void {
  apiOverride = api;
}
