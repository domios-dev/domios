/**
 * Roles require a verified session binding: `set_tab_role` and `make_orchestrator`
 * both pass `require_verified_session: true`, so a plain shell tab can never hold
 * one. A spawned agent reports its binding through the session_start hook a moment
 * after the PTY comes up, which is why every role assignment waits for it here
 * instead of racing the hook.
 */
import { pollUntil } from "./poll";
import type { ApiClient } from "./api-client";

export async function waitForSessionBinding(
  api: ApiClient,
  tabId: string,
  timeout = 120_000,
): Promise<void> {
  // Mux state rather than `api.tabs()`: the tab list is scoped to the ACTIVE
  // workspace, so an agent spawned into a fixture workspace is simply absent
  let last = "never observed";
  await pollUntil(
    async () => {
      const state = await api
        .get<{ tabs?: Record<string, { agent_session_id?: string | null }> }>("/mux/state")
        .catch(() => ({ tabs: {} }));
      const tab = state.tabs?.[tabId];
      last = tab ? `session=${tab.agent_session_id}` : "tab absent from mux state";
      return Boolean(tab?.agent_session_id);
    },
    timeout,
    250,
    `session binding for tab ${tabId}`,
  ).catch(() => {
    throw new Error(`session binding never arrived for tab ${tabId}; last seen: ${last}`);
  });
}

/** Spawn an agent and hand back a tab that can immediately take a role. */
export async function spawnBoundAgent<T extends { id?: string; tab_id?: string }>(
  api: ApiClient,
  body: Record<string, unknown>,
): Promise<string> {
  const spawned = await api.post<T>("/agents/spawn", { agent: "claude", skip_permissions: true, ...body });
  const tabId = spawned.tab_id ?? spawned.id;
  if (!tabId) throw new Error(`spawn returned no tab id: ${JSON.stringify(spawned)}`);
  await waitForSessionBinding(api, tabId);
  return tabId;
}
