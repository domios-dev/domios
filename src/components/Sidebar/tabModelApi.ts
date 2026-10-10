import { invoke } from "../../lib/bridge";

export interface TabModelApi {
  cancelStagedModel?(tabId: string): Promise<void>;
  switchModel(tabId: string, modelId: string, reasoningEffort?: string, runtimeMode?: string): Promise<ModelSwitchResult | void>;
  restartAndSwitchModel?(tabId: string, modelId: string, reasoningEffort?: string): Promise<ModelSwitchResult | void>;
}

export interface ModelSwitchResult {
  staged?: boolean;
  mode_used: "live" | "live_command" | "next_turn" | "restart_resume";
  model_confirmation: "observed" | "pending";
}

const defaultApi: TabModelApi = {
  cancelStagedModel: (tabId) => invoke("switch_agent_model", { tabId, model: "", cancel: true }),
  restartAndSwitchModel: (tabId, model, reasoningEffort) =>
    invoke<ModelSwitchResult>("switch_agent_model", { tabId, model, reasoningEffort, restartResume: true }),
  switchModel: async (tabId, model, reasoningEffort, runtimeMode) => {
    return invoke<ModelSwitchResult>("switch_agent_model", { tabId, model, reasoningEffort, runtimeMode });
  },
};

let apiOverride: TabModelApi | null = null;

export function getTabModelApi(): TabModelApi {
  return apiOverride ?? defaultApi;
}

export function __setTabModelApiForTest(api: TabModelApi | null): void {
  apiOverride = api;
}
