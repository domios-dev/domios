/** Data seam for chatBackend session state and model switching. Catalog entries
 * come from the shared provider descriptors consumed by ChatModelBubble. */
import { invoke } from "../../lib/bridge";

/** Mirror of the Rust `ChatSessionInfo` — the model the live session talks to. */
export interface ChatSessionInfo {
  model: string | null;
}

export interface ChatModelBubbleApi {
  sessionInfo(pluginId: string, sessionId: string): Promise<ChatSessionInfo>;
  switchModel(pluginId: string, sessionId: string, model: string): Promise<ChatSessionInfo>;
}

const defaultApi: ChatModelBubbleApi = {
  sessionInfo: (pluginId, sessionId) =>
    invoke<ChatSessionInfo>("chatbackend_session_info", { pluginId, sessionId }),
  switchModel: (pluginId, sessionId, model) =>
    invoke<ChatSessionInfo>("chatbackend_switch_model", { pluginId, sessionId, model }),
};

let apiOverride: ChatModelBubbleApi | null = null;

export function getChatModelBubbleApi(): ChatModelBubbleApi {
  return apiOverride ?? defaultApi;
}

export function __setChatModelBubbleApiForTest(api: ChatModelBubbleApi | null): void {
  apiOverride = api;
}
