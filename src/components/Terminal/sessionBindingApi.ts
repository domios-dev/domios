import { invoke, IS_WEB } from "../../lib/bridge";
import {
  daemonAuthToken,
  daemonHttpBase,
  type DaemonApiConfig,
} from "../../lib/webApi";

interface ConfigSnapshot {
  api?: DaemonApiConfig;
}

export async function rebindTabSession(tabId: string, sessionId: string): Promise<void> {
  if (IS_WEB) {
    await invoke("rebind_tab_session", { tabId, sessionId });
    return;
  }
  const config = await invoke<ConfigSnapshot>("get_config");
  const response = await fetch(
    `${daemonHttpBase(config.api)}/api/tabs/${encodeURIComponent(tabId)}/session/rebind`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${daemonAuthToken(config.api)}`,
      },
      body: JSON.stringify({ session_id: sessionId }),
    },
  );
  if (!response.ok) throw new Error((await response.text()) || `Rebind failed (${response.status})`);
}
