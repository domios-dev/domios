/**
 * Data seam for the chatBackend shell model bubble (CB-P3).
 *
 * A shell pane has no agent registry entry — its provider is `backing.plugin_id`
 * and its switchable models live in the plugin snapshot (CB-P1 joins them per
 * plugin). The live model + model switching go through the chatBackend commands.
 * Core asks the capability resolved by `plugin_id`; never a hardcoded id.
 */
import { invoke } from "../../lib/bridge";
import type { ModelInfo } from "../../lib/types";

/** Mirror of the Rust `ChatSessionInfo` — the model the live session talks to. */
export interface ChatSessionInfo {
  model: string | null;
}

export interface ChatModelBubbleApi {
  sessionInfo(pluginId: string, sessionId: string): Promise<ChatSessionInfo>;
  availableModels(pluginId: string): Promise<ModelInfo[]>;
  switchModel(pluginId: string, sessionId: string, model: string): Promise<ChatSessionInfo>;
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
};

let apiOverride: ChatModelBubbleApi | null = null;

export function getChatModelBubbleApi(): ChatModelBubbleApi {
  return apiOverride ?? defaultApi;
}

export function __setChatModelBubbleApiForTest(api: ChatModelBubbleApi | null): void {
  apiOverride = api;
}
