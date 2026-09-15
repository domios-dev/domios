import {
  IS_WEB,
  webFetch,
  daemonHttpBase,
  daemonAuthToken,
  type DaemonApiConfig,
} from "./webApi";

/** Daemon-owned helper spawn: provider, General placement and cwd are resolved server-side. */
export const AI_HELPER_SPAWN_PATH = "/api/agents/helper";

export interface AiHelperRequest {
  title: string;
  task: string;
}

export type AiHelperDeliveryState = "confirmed" | "launched";

export interface AiHelperSpawned {
  tabId: string;
  workspaceId: string | null;
  deliveryState: AiHelperDeliveryState;
}

type AiHelperPayload = {
  id?: string;
  tab_id?: string;
  workspace_id?: string;
  delivery_state?: string;
  error?: string | { message?: string };
} | null;

const DELIVERY_STATES: readonly string[] = ["confirmed", "launched"];

function postHelper(request: AiHelperRequest, api: DaemonApiConfig): Promise<Response> {
  const init = {
    method: "POST",
    body: JSON.stringify({ title: request.title, task: request.task }),
  };
  if (IS_WEB) return webFetch(AI_HELPER_SPAWN_PATH, init);
  const token = daemonAuthToken(api);
  return fetch(`${daemonHttpBase(api)}${AI_HELPER_SPAWN_PATH}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
  });
}

export async function spawnAiHelper(
  request: AiHelperRequest,
  api: DaemonApiConfig = {},
): Promise<AiHelperSpawned> {
  const response = await postHelper(request, api);
  const payload = (await response.json().catch(() => null)) as AiHelperPayload;
  if (!response.ok) {
    const error = payload?.error;
    throw new Error(
      typeof error === "string"
        ? error
        : error?.message ?? `AI helper launch failed with HTTP ${response.status}`,
    );
  }
  const tabId = payload?.id ?? payload?.tab_id;
  if (!tabId) throw new Error("AI helper launch returned no tab id");
  const deliveryState = payload?.delivery_state;
  if (!deliveryState || !DELIVERY_STATES.includes(deliveryState)) {
    throw new Error(`AI helper launch returned an unknown delivery state: ${deliveryState ?? "none"}`);
  }
  window.dispatchEvent(new CustomEvent("codeterm:focus-tab", { detail: { tabId } }));
  return {
    tabId,
    workspaceId: payload?.workspace_id ?? null,
    deliveryState: deliveryState as AiHelperDeliveryState,
  };
}
