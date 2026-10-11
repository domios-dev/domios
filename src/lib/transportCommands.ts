import { createTabsTransport, type PtyCursors } from './transport/tabs';
import { createCapabilitiesTransport } from './transport/capabilities';
import { createOrchestrationTransport } from './transport/orchestration';
import { setClientTabUiMode } from './clientTabUiMode';
import type { TransportClient } from './transportClient';
export type CommandArgs = Record<string, unknown>;
export type CommandAdapter = (args: CommandArgs) => Promise<unknown>;
const enc = encodeURIComponent;
const s = (args: CommandArgs, key: string): string => String(args[key] ?? '');
export const CLIENT_LOCAL_COMMANDS = ['mux_set_chat_view', 'kill_pty'] as const;
export const REFUSED_COMMANDS: Record<string, string> = {};
export const CANNED_COMMANDS = { check_cli_installed: true };
export function createCommandRegistry(client: TransportClient, cursors?: PtyCursors): Record<string, CommandAdapter> {
  const webJson = client.json.bind(client);
  const extra: Record<string, CommandAdapter> = {
    notify: (a) =>
      webJson("POST", "/api/notifications", {
        title: a.title,
        body: a.body,
        level: a.level,
        tab_id: a.tabId,
        workspace_id: a.workspaceId,
        category: a.category,
        event_key: a.eventKey,
      }),
    mux_clear_tab_role: (a) =>
      webJson("DELETE", `/api/tabs/${enc(s(a, "tabId"))}/role`),
    mux_set_chat_view: async (a) => {
      setClientTabUiMode(s(a, "tabId"), a.chatView ?? a.enabled ? "chat" : "terminal");
      return { ok: true };
    },
    get_tab_output: async (a) => {
      const resp = await webJson<{ output?: string }>(
        "GET",
        `/api/tabs/${enc(s(a, "tabId"))}/output`,
      );
      return resp.output ?? "";
    },
    get_remote_workers: () => webJson("GET", "/api/remote-workers"),
    restart_general_agent_tab: (a) =>
      webJson("POST", `/api/agents/general/${enc(String(a.tabId ?? a.tab_id ?? ""))}/restart`),
    copy_image_to_clipboard: (a) =>
      webJson("POST", "/api/clipboard/copy-image", { path: a.path }),
    restart_app: () => webJson("POST", "/api/app/restart"),
    frontend_readiness_generation: async () => (await client.localJson<{ generation: number }>('GET', '/api/frontend/bootstrap')).generation,
    frontend_ready: (a) => client.localJson('POST', '/api/frontend/ready', { generation: a.generation }),
    get_recovery_state: () => client.localJson('GET', '/api/desktop/recovery'),
    get_rest_api_status: () => client.localJson('GET', '/api/listener/status'),
    restart_rest_api: () => client.localJson('POST', '/api/listener/restart'),
    kill_pty: async () => null,
  };
  const registry: Record<string, CommandAdapter> = Object.assign(Object.create(null), createTabsTransport({
    json: (method, path, body, opts) =>
      path === '/api/mesh/relay-to' || path === '/api/mesh/peers'
        ? client.localJson(method, path, body, opts)
        : client.json(method, path, body, opts)
  }, cursors), createCapabilitiesTransport(client), createOrchestrationTransport(client), extra);
  return registry;
}
export function dispatchCommand<T>(registry: Record<string, CommandAdapter>, command: string, args: CommandArgs = {}): Promise<T> {
  if (!Object.prototype.hasOwnProperty.call(registry, command)) return Promise.reject(new Error('No audited HTTP route for command: ' + command));
  return registry[command](args) as Promise<T>;
}
