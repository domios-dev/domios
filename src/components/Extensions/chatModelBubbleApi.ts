/** Data seam for chatBackend session state and model switching. Catalog entries
 * come from the shared provider descriptors consumed by ChatModelBubble. */
import { invoke } from "../../lib/bridge";

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

const defaultApi: ChatModelBubbleApi = {
  sessionInfo: (pluginId, sessionId) =>
    invoke<ChatSessionInfo>("chatbackend_session_info", { pluginId, sessionId }),
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
