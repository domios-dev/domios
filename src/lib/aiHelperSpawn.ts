import {
  IS_WEB,
  webFetch,
  daemonHttpBase,
  daemonAuthToken,
  type DaemonApiConfig,
} from "./webApi";

/** Daemon-owned helper spawn: provider, General placement and cwd are resolved server-side. */
export const AI_HELPER_SPAWN_PATH = "/api/agents/helper";

/** Context every helper task carries: the user launched it and is reading this tab. */
export const AI_HELPER_CONTEXT: readonly string[] = [
  "The user opened this helper tab by clicking a setup button and is reading this chat right now.",
  "Ask every question here in chat and wait for the answer in chat. Do not ask through `codeterm notify`, notification choices, or a provider question tool; this tab is where the user is.",
  "The click that opened this tab is the user's consent to the setup it names, including installing a pinned runtime the setup guide requires. Do not ask for that consent again; ask only for values only the user knows.",
];

export function withAiHelperContext(task: string): string {
  return `${AI_HELPER_CONTEXT.join("\n")}\n\n${task}`;
}

export interface AiHelperRequest {
  title: string;
  description: string;
  task: string;
}

export type AiHelperDeliveryState = "confirmed" | "launched";

export interface AiHelperSpawned {
  tabId: string;
  workspaceId: string;
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
    body: JSON.stringify({ title: request.title, description: request.description, task: request.task }),
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
  const workspaceId = payload?.workspace_id;
  if (!workspaceId) throw new Error("AI helper launch returned no workspace id");
  window.dispatchEvent(new CustomEvent("codeterm:focus-tab", { detail: { tabId, workspaceId } }));
  return {
    tabId,
    workspaceId,
    deliveryState: deliveryState as AiHelperDeliveryState,
  };
}
