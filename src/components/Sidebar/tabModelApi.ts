/** Data seam for the sidebar row's model switch. The daemon relaunches the tab
 *  on its own session with the new model and resolves once the provider
 *  reports that model — never a slash command typed into the agent. */
import { invoke } from "../../lib/bridge";

export interface TabModelApi {
  switchModel(tabId: string, modelId: string): Promise<void>;
}

const defaultApi: TabModelApi = {
  switchModel: async (tabId, model) => {
    await invoke("switch_agent_model", { tabId, model });
  },
};

let apiOverride: TabModelApi | null = null;

export function getTabModelApi(): TabModelApi {
  return apiOverride ?? defaultApi;
}

export function __setTabModelApiForTest(api: TabModelApi | null): void {
  apiOverride = api;
}
