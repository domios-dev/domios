import { invoke, IS_WEB } from "../../lib/bridge";
import { localFetch } from "../../lib/webApi";

export async function rebindTabSession(tabId: string, sessionId: string): Promise<void> {
  if (IS_WEB) {
    await invoke("rebind_tab_session", { tabId, sessionId });
    return;
  }
  const response = await localFetch(
    `/api/tabs/${encodeURIComponent(tabId)}/session/rebind`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ session_id: sessionId }),
    },
  );
  if (!response.ok) throw new Error((await response.text()) || `Rebind failed (${response.status})`);
}
