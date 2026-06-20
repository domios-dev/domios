/**
 * Data seam for the chatBackend shell model bubble. A shell has no agent registry
 * entry — its provider is `backing.plugin_id`; live model + switching go through the
 * chatBackend commands, resolved by `plugin_id`, never a hardcoded id.
 */
import { invoke } from "../../lib/bridge";
import type { ModelInfo } from "../../lib/types";

/** Mirror of the Rust `ChatSessionInfo` — the model the live session talks to. */
export interface ChatSessionInfo {
  model: string | null;
}

/**
 * The plugin's verdict on a pending model switch: whether it needs confirmation and
 * the message to show. The UI stays generic — no provider/model logic here.
 */
export interface ModelSwitchDescription {
  needsConfirm: boolean;
  message: string;
}

export interface ChatModelBubbleApi {
  sessionInfo(pluginId: string, sessionId: string): Promise<ChatSessionInfo>;
  availableModels(pluginId: string): Promise<ModelInfo[]>;
  switchModel(pluginId: string, sessionId: string, model: string): Promise<ChatSessionInfo>;
  /**
   * Ask the plugin whether switching to `model` needs confirmation (+ the message).
   * Optional: an unimplemented/rejecting backend falls back to a direct switch.
   */
  describeModelSwitch?(
    pluginId: string,
    sessionId: string,
    model: string,
  ): Promise<ModelSwitchDescription | null>;
}

interface SnapshotPlugin {
  id: string;
  available_models?: ModelInfo[] | null;
}

const defaultApi: ChatModelBubbleApi = {
  sessionInfo: (pluginId, sessionId) =>
    invoke<ChatSessionInfo>("chatbackend_session_info", { pluginId, sessionId }),
  availableModels: async (pluginId) => {
    const snap = await invoke<{ plugins?: SnapshotPlugin[] }>("plugin_manager_snapshot");
    return (snap?.plugins ?? []).find((p) => p.id === pluginId)?.available_models ?? [];
  },
  switchModel: (pluginId, sessionId, model) =>
    invoke<ChatSessionInfo>("chatbackend_switch_model", { pluginId, sessionId, model }),
  describeModelSwitch: async (pluginId, sessionId, model) => {
    try {
      return await invoke<ModelSwitchDescription | null>("chatbackend_describe_model_switch", {
        pluginId,
        sessionId,
        model,
      });
    } catch {
      // Plugin/backend doesn't expose the hook → no crash, caller switches directly.
      return null;
    }
  },
};

let apiOverride: ChatModelBubbleApi | null = null;

export function getChatModelBubbleApi(): ChatModelBubbleApi {
  return apiOverride ?? defaultApi;
}

export function __setChatModelBubbleApiForTest(api: ChatModelBubbleApi | null): void {
  apiOverride = api;
}
