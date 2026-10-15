import { invoke } from "./bridge";
import { createSessionModelResource } from "./shellSessionModelResource";

interface ChatSessionInfo {
  model?: string | null;
}

const resource = createSessionModelResource(async (pluginId, sessionId) => {
  const info = await invoke<ChatSessionInfo>("chatbackend_session_info", { pluginId, sessionId });
  return info?.model ?? null;
});

export function getCachedShellModel(
  pluginId: string | null | undefined,
  sessionId: string | null | undefined,
): string | null {
  return pluginId && sessionId ? resource.get(pluginId, sessionId) : null;
}

export const subscribeShellModel = resource.subscribe;
export const fetchShellSessionModel = resource.load;
