/** Data seam for the sidebar row's model switch. The daemon relaunches the tab
 *  on its own session with the new model and resolves once the provider
 *  reports that model — never a slash command typed into the agent. */
import { invoke } from "../../lib/bridge";

export interface TabModelApi {
  switchModel(tabId: string, modelId: string, reasoningEffort?: string): Promise<ModelSwitchResult | void>;
}

export interface ModelSwitchResult {
  mode_used: "live" | "restart_resume";
  model_confirmation: "observed" | "pending";
}

const defaultApi: TabModelApi = {
  switchModel: async (tabId, model, reasoningEffort) => {
    return invoke<ModelSwitchResult>("switch_agent_model", { tabId, model, reasoningEffort });
  },
};

let apiOverride: TabModelApi | null = null;

export function getTabModelApi(): TabModelApi {
  return apiOverride ?? defaultApi;
}

export function __setTabModelApiForTest(api: TabModelApi | null): void {
  apiOverride = api;
}
