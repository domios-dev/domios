/**
 * Tauri ↔ HTTP bridge — the web mirror's IPC layer.
 *
 * Every `invoke()`/`listen()` in src/ imports from here instead of
 * `@tauri-apps/api`. In the Tauri webview both delegate to the real APIs
 * (zero behavior change). In WEB mode (`IS_WEB`) invoke() routes through
 * the daemon's REST API via a command map, and listen() rides the
 * daemon's SSE event stream (with a polling tick as belt-and-braces).
 *
 * Unmapped commands fail LOUDLY with a clear error — honest errors over
 * silent fallbacks. Commands that are meaningless in a browser (window
 * management, clipboard-native, autostart) return canned defaults.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import {
  listen as tauriListen,
  type UnlistenFn as TauriUnlistenFn,
} from "@tauri-apps/api/event";
import { currentToken, IS_WEB, WebApiError, webFetch, webJson } from "./webApi";
import { webListen } from "./webEvents";
import { WEB_SPECIAL, WEB_UNSUPPORTED } from "./bridgeWebUnsupported";
import { setClientTabUiMode } from "./clientTabUiMode";
import { checkBuildMismatch } from "./buildMismatchGuard";
import type { TabInfo } from "./types";

export type UnlistenFn = TauriUnlistenFn;
export { IS_WEB };

type Args = Record<string, unknown>;
type Mapper = (args: Args) => Promise<unknown>;
const webPtyOffsets = new Map<string, number>();
const webPtyTruncations = new Map<string, number>();
let invokeForTest: (<T>(cmd: string, args?: Args) => Promise<T>) | null = null;
let listenForTest:
  | (<T>(event: string, handler: (e: { payload: T }) => void) => Promise<UnlistenFn>)
  | null = null;

export function __setInvokeForTest(fn: (<T>(cmd: string, args?: Args) => Promise<T>) | null): void {
  invokeForTest = fn;
}

export function __setListenForTest(
  fn: (<T>(event: string, handler: (e: { payload: T }) => void) => Promise<UnlistenFn>) | null,
): void {
  listenForTest = fn;
}

/** Drop the tail cursor so the next read_pty replays retained history from 0. */
export function resetWebPtyTail(ptyId: string): void {
  webPtyOffsets.delete(ptyId);
  webPtyTruncations.delete(ptyId);
}

/** Consume (once) the under-retention marker from the last `read_pty` response,
 * if the ring evicted bytes ahead of the client's cursor. */
export function takePtyTruncation(ptyId: string): number | null {
  const from = webPtyTruncations.get(ptyId);
  if (from === undefined) return null;
  webPtyTruncations.delete(ptyId);
  return from;
}

/** True iff a `read_pty` failure means the PTY is confirmed gone — a stable,
 * structured signal `usePtyConnection` can trust to respawn, as opposed to a
 * transient transport hiccup that should just retry. */
export function isPtyDeadError(err: unknown): boolean {
  if (IS_WEB) return err instanceof WebApiError && err.status === 404;
  const message = err instanceof Error ? err.message : String(err);
  return message.includes("PTY not found") || message.includes("PTY closed");
}

const enc = encodeURIComponent;
const s = (args: Args, key: string): string => String(args[key] ?? "");

async function webJsonWithCallerTab<T>(
  method: string,
  path: string,
  body: unknown,
  callerTabId: unknown,
): Promise<T> {
  const headers = new Headers();
  if (callerTabId) headers.set("X-CodeTerm-Tab-Id", String(callerTabId));
  const response = await webFetch(path, {
    method,
    headers,
    body: JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) throw new WebApiError(response.status, path, text);
  return JSON.parse(text) as T;
}

/** Commands with a meaningful HTTP equivalent. */
const HTTP_MAP: Record<string, Mapper> = {
  plugin_manager_snapshot: () => webJson("GET", "/api/plugins/snapshot"),
  plugin_manager_action: (a) => webJson("POST", "/api/plugins/action", a.action),
  plugin_glance: (a) => webJson("POST", "/api/plugins/glance", a),
  plugin_bubble: (a) => webJson("POST", "/api/plugins/bubble", a),
  plugin_view_call: (a) => webJson("POST", "/api/plugins/view-call", a),
  chatbackend_session_info: (a) =>
    webJson("POST", "/api/plugins/chatbackend/session-info", {
      pluginId: s(a, "pluginId"),
      sessionId: s(a, "sessionId"),
    }),
  chatbackend_switch_model: (a) =>
    webJson("POST", "/api/plugins/chatbackend/switch-model", {
      pluginId: s(a, "pluginId"),
      sessionId: s(a, "sessionId"),
      model: s(a, "model"),
    }),
  chatbackend_describe_model_switch: (a) =>
    webJson("POST", "/api/plugins/chatbackend/describe-model-switch", {
      pluginId: s(a, "pluginId"),
      sessionId: s(a, "sessionId"),
      model: s(a, "model"),
    }),
  open_chat_backend_tab: (a) =>
    webJson("POST", "/api/plugins/chatbackend/open", {
      pluginId: s(a, "pluginId"),
      workspaceId: a.workspaceId,
      config: a.config,
      systemPrompt: a.systemPrompt,
      preset: a.preset,
      model: a.model,
    }),
  get_config: async () => {
    const config = await webJson<Record<string, unknown>>("GET", "/api/config");
    const api = config.api as Record<string, unknown> | undefined;
    if (api) api.token = currentToken();
    return config;
  },
  list_notifications: () => webJson("GET", "/api/notifications"),
  notify: (a) =>
    webJson("POST", "/api/notifications", {
      title: a.title,
      body: a.body,
      level: a.level,
      tab_id: a.tabId,
      workspace_id: a.workspaceId,
    }),
  notify_agent_input_request: (a) =>
    webJson("POST", "/api/notifications/input-request", {
      tab_id: a.tabId,
      session_id: a.sessionId,
      prompt_identity: a.promptIdentity,
      resolved: a.resolved,
    }),
  mark_notification_read: (a) =>
    webJson("POST", `/api/notifications/${enc(s(a, "id"))}/read`),
  clear_notifications: (a) =>
    a.id
      ? webJson("DELETE", `/api/notifications/${enc(s(a, "id"))}`)
      : webJson("POST", "/api/notifications/clear"),
  // PATCH: the route has no POST; the handler merges top-level keys, so the full config object works as a partial.
  update_config: (a) => webJson("PATCH", "/api/config", a.config),
  get_network_info: () => webJson("GET", "/api/network"),
  get_serve_status: () => webJson("GET", "/api/network/serve/status"),
  enable_web_access: (a) =>
    webJson("POST", "/api/network/serve/enable", { https_port: a.https_port }),
  disable_web_access: () => webJson("POST", "/api/network/serve/disable"),

  mux_get_state: () => webJson("GET", "/api/mux/state"),
  mux_set_active_workspace: (a) =>
    webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/focus`),
  mux_create_workspace: (a) =>
    webJson("POST", "/api/workspaces", {
      name: a.name,
      cwd: a.cwd,
      activate: a.activate,
    }),
  mux_close_workspace: (a) =>
    webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/close`),
  mux_rename_workspace: (a) =>
    webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/rename`, {
      name: a.name,
    }),
  mux_rename_team: (a) =>
    webJsonWithCallerTab(
      "POST",
      `/api/teams/${enc(s(a, "teamId"))}/rename`,
      { name: a.name },
      a.callerTabId,
    ),
  mux_close_team: (a) =>
    webJsonWithCallerTab(
      "POST",
      `/api/teams/${enc(s(a, "teamId"))}/close`,
      {},
      a.callerTabId,
    ),
  mux_add_team_member: (a) =>
    webJsonWithCallerTab(
      "POST",
      `/api/teams/${enc(s(a, "teamId"))}/members`,
      {
        provider: a.provider,
        role: a.role,
        title: a.title,
        task: a.task,
        model: a.model,
      },
      a.callerTabId,
    ),
  mux_create_tab: (a) =>
    webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs`, {
      eager: a.eager === true,
    }),
  mux_set_active_tab: (a) =>
    webJson(
      "POST",
      `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/focus`,
    ),
  mux_set_active_tab_and_workspace: async (a) => {
    await webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/focus`);
    return webJson(
      "POST",
      `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/focus`,
    );
  },
  mux_close_tab: (a) =>
    webJson(
      "POST",
      `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/close`,
    ),
  mux_reopen_closed_tab: (a) =>
    webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/reopen-closed`, {
      index: a.index,
    }),
  mux_reopen_closed_group: (a) =>
    webJson("POST", `/api/closed-groups/${enc(s(a, "groupId"))}/reopen`),
  mux_restore_orphaned_tab: (a) =>
    webJson("POST", `/api/mux/restore-orphaned`, { tab_id: s(a, "tabId") }),
  mux_rename_tab: (a) =>
    webJson(
      "POST",
      `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/rename`,
      { name: a.name },
    ),
  mux_move_tab: (a) =>
    webJson(
      "POST",
      `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/move`,
      a,
    ),
  mux_reorder_tab: (a) =>
    webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/reorder`, {
      ordered_tab_ids: a.orderedTabIds,
    }),
  mux_reorder_child: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "parentTabId"))}/children/reorder`, {
      ordered_tab_ids: a.orderedTabIds,
    }),
  mux_reorder_workspace: (a) =>
    webJson("POST", `/api/workspaces/reorder`, {
      ordered_workspace_ids: a.orderedWorkspaceIds,
    }),
  mux_set_focused_tab: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/focus`),
  mux_respond_prompt: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/prompt/respond`, a),
  mux_set_tab_agent_type: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent-type`, {
      agent_type: a.agentType,
    }),
  mux_clear_tab_role: (a) =>
    webJson("DELETE", `/api/tabs/${enc(s(a, "tabId"))}/role`),
  mux_make_orchestrator: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/make-orchestrator`),
  mux_make_planner: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/make-planner`, {
      plan_id: s(a, "planId"),
    }),
  mux_set_chat_view: async (a) => {
    setClientTabUiMode(s(a, "tabId"), a.chatView ?? a.enabled ? "chat" : "terminal");
    return { ok: true };
  },

  // ── tabs (read + input) ── REST returns JSON envelopes; Tauri commands return bare strings — unwrap.
  get_tab_text: async (a) => {
    const resp = await webJson<{ text?: string }>(
      "GET",
      `/api/tabs/${enc(s(a, "tabId"))}/text?formatted=${a.formatted ? "true" : "false"}`,
    );
    return resp.text ?? "";
  },
  get_tab_output: async (a) => {
    const resp = await webJson<{ output?: string }>(
      "GET",
      `/api/tabs/${enc(s(a, "tabId"))}/output`,
    );
    return resp.output ?? "";
  },
  shell_command_snapshot: (a) =>
    webJson("GET", `/api/tabs/${enc(s(a, "tabId"))}/shell-commands`),

  chat_start: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/start`, {
      subscriptionId: s(a, "subscriptionId"),
    }),
  chat_stop: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/stop`, {
      subscriptionId: s(a, "subscriptionId"),
    }),
  chat_backfill: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/backfill`, {
      before_offset: a.beforeOffset,
      max_bytes: a.maxBytes,
    }),
  session_transcript: (a) =>
    webJson(
      "GET",
      `/api/sessions/transcript?provider=${enc(s(a, "provider"))}&cwd=${enc(s(a, "cwd"))}&session_id=${enc(s(a, "sessionId"))}`,
    ),
  chat_send_text: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send`, {
      text: a.text,
    }),
  chat_send_status: (a) =>
    webJson("GET", `/api/tabs/${enc(s(a, "tabId"))}/chat-send`),
  delayed_send_list: (a) =>
    webJson("GET", `/api/tabs/${enc(s(a, "tabId"))}/delayed-messages`),
  delayed_send_create: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/delayed-messages`, {
      text: a.text,
      scheduledAtMs: a.scheduledAtMs,
    }),
  delayed_send_update: (a) =>
    webJson("PATCH", `/api/delayed-messages/${enc(s(a, "id"))}`, {
      text: a.text,
      scheduledAtMs: a.scheduledAtMs,
    }),
  delayed_send_delete: (a) =>
    webJson("DELETE", `/api/delayed-messages/${enc(s(a, "id"))}`),
  delayed_send_send_now: (a) =>
    webJson("POST", `/api/delayed-messages/${enc(s(a, "id"))}/send-now`),
  transcribe_voice_attachment: async (a) => {
    const resp = await webJson<{ line?: string }>("POST", "/api/transcribe-voice", {
      path: s(a, "path"),
    });
    return resp.line ?? "";
  },

  mesh_get_status: () => webJson("GET", "/api/mesh/status"),
  mesh_list_peers: () => webJson("GET", "/api/mesh/peers"),
  mesh_peer_tabs_list: (a) => webJson("POST", "/api/mesh/relay-to", {
    target_peer_id: s(a, "instanceId") ?? s(a, "instance_id"),
    method: "GET",
    path: "/api/tabs?all_workspaces=true",
    body: null,
  }).then((response: unknown) => {
    const relay = response as { ok?: boolean; data?: unknown; error?: string | null };
    if (!relay.ok) throw new Error(relay.error ?? "Remote tabs unavailable");
    return relay.data;
  }),
  mesh_remote_tab_action: (a) => {
    const peer = s(a, "peerId") ?? s(a, "peer_id");
    const ordinary = s(a, "ordinaryTabId") ?? s(a, "ordinary_tab_id");
    const prefix = `mesh:${peer}/`;
    if (!ordinary.startsWith(prefix) || ordinary.slice(prefix.length).includes("/")) {
      return Promise.reject(new Error("Remote tab owner mismatch"));
    }
    const remoteTab = ordinary.slice(prefix.length);
    const suffix = s(a, "suffix");
    const path = suffix === "/inspect"
      ? `/api/tabs/inspect?tab_ids=${enc(remoteTab)}`
      : `/api/tabs/${enc(remoteTab)}${suffix}`;
    return webJson("POST", "/api/mesh/relay-to", {
      target_peer_id: peer,
      method: s(a, "method"),
      path,
      body: a.body ?? null,
    }).then((response: unknown) => {
      const relay = response as { ok?: boolean; data?: unknown; error?: string | null };
      if (!relay.ok) throw new Error(relay.error ?? "Remote tab unavailable");
      return relay.data;
    });
  },
  mesh_peer_services_list: (a) =>
    webJson("GET", `/api/mesh/peers/${enc(s(a, "instanceId"))}/services`),
  mesh_peer_service_delete: (a) =>
    webJson(
      "DELETE",
      `/api/mesh/peers/${enc(s(a, "instanceId"))}/services/${enc(s(a, "id"))}`,
    ),
  mesh_peer_cron_action: (a) =>
    webJson(
      "POST",
      `/api/mesh/peers/${enc(s(a, "instanceId"))}/crons/${enc(s(a, "id"))}/${enc(s(a, "action"))}`,
    ),
  mesh_peer_cron_get: (a) =>
    webJson(
      "GET",
      `/api/mesh/peers/${enc(s(a, "instanceId"))}/crons/${enc(s(a, "id"))}`,
    ),
  mesh_peer_cron_history: (a) =>
    webJson(
      "GET",
      `/api/mesh/peers/${enc(s(a, "instanceId"))}/crons/${enc(s(a, "id"))}/history`,
    ),
  mesh_scan_peers: () => webJson("POST", "/api/mesh/discover"),
  mesh_connect: (a) =>
    webJson("POST", "/api/mesh/connect", { instance_id: a.instanceId }),
  mesh_disconnect: (a) =>
    webJson("POST", `/api/mesh/peers/disconnect/${enc(s(a, "instanceId"))}`),
  mesh_approve_pairing: (a) =>
    webJson("POST", "/api/mesh/approve", { request_id: a.requestId }),
  mesh_reject_pairing: (a) =>
    webJson("POST", "/api/mesh/reject", { request_id: a.requestId }),
  mesh_add_manual: (a) => webJson("POST", "/api/mesh/peers/add-manual", a),
  mesh_set_enabled: (a) => webJson("POST", "/api/mesh/enabled", { enabled: a.enabled }),
  mesh_start_broadcast: () => webJson("POST", "/api/mesh/broadcast/start"),
  mesh_stop_broadcast: () => webJson("POST", "/api/mesh/broadcast/stop"),
  mesh_exit: () => webJson("POST", "/api/mesh/exit"),
  mesh_set_access: (a) =>
    webJson("POST", `/api/mesh/peers/${enc(s(a, "instanceId"))}/access`, {
      access_level: a.accessLevel,
    }),

  services_list: () => webJson("GET", "/api/services"),
  services_create: (a) => webJson("POST", "/api/services", a.input),
  services_update: (a) =>
    webJson("PATCH", `/api/services/${enc(s(a, "id"))}`, a.patch),
  services_delete: (a) => webJson("DELETE", `/api/services/${enc(s(a, "id"))}`),
  services_regenerate_token: (a) =>
    webJson("POST", `/api/services/${enc(s(a, "id"))}/regenerate-token`),
  activity_list: () => webJson("GET", "/api/activity"),
  cron_list: (a) => webJson("GET", `/api/crons${a.workspaceId ? `?workspace_id=${enc(s(a, "workspaceId"))}` : ""}`),
  cron_get: (a) => webJson("GET", `/api/crons/${enc(s(a, "id"))}`),
  cron_history: (a) => webJson("GET", `/api/crons/${enc(s(a, "id"))}/history`),
  cron_delete: (a) => webJson("DELETE", `/api/crons/${enc(s(a, "id"))}`),
  cron_trigger: (a) => webJson("POST", `/api/crons/${enc(s(a, "id"))}/trigger`),
  cron_toggle: (a) =>
    webJson(
      "POST",
      `/api/crons/${enc(s(a, "id"))}/${a.enabled ? "enable" : "disable"}`,
    ),
  cron_update: (a) => webJson("PATCH", `/api/crons/${enc(s(a, "id"))}`, a.params ?? a),
  services_smart_add_inspect: (a) =>
    webJson("POST", "/api/services/smart-add-inspect", { url: a.url }),

  spawn_pty: async (a) => {
    const resp = await webJson<{ pty_id: string }>("POST", "/api/pty/spawn", {
      command: a.command,
      cwd: a.cwd,
      cols: a.cols,
      rows: a.rows,
      predicted_id: a.predictedId,
      tab_id: a.tabId,
    });
    return resp.pty_id;
  },
  read_pty: async (a) => {
    const ptyId = s(a, "ptyId");
    // First attach tails from 0 — the initial read doubles as history replay.
    const offset = webPtyOffsets.get(ptyId) ?? 0;
    const resp = await webJson<{ data: number[]; offset: number; truncated_from?: number }>(
      "GET",
      `/api/pty/${enc(ptyId)}/read?offset=${offset}`,
    );
    webPtyOffsets.set(ptyId, resp.offset);
    if (resp.truncated_from !== undefined) webPtyTruncations.set(ptyId, resp.truncated_from);
    return resp.data;
  },
  write_pty_string: (a) =>
    webJson("POST", `/api/pty/${enc(s(a, "ptyId"))}/write`, {
      data: a.data,
      raw: a.raw !== false,
    }),
  interrupt_agent: (a) =>
    webJson("POST", `/api/pty/${enc(s(a, "ptyId"))}/write`, {
      data: "\x1b",
      raw: true,
    }),
  resize_pty: (a) => {
    if (!a.clientId) return Promise.resolve({ ok: true, ignored: true });
    return webJson("POST", `/api/pty/${enc(s(a, "ptyId"))}/resize`, {
      cols: a.cols,
      rows: a.rows,
      client_id: a.clientId,
    });
  },
  claim_resize_owner: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/resize-owner`, {
      pty_id: a.ptyId,
      client_id: a.clientId,
      cols: a.cols,
      rows: a.rows,
      force: a.force === true,
    }),

  keepawake_status: () => webJson("GET", "/api/keepawake"),
  keepawake_start: (a) =>
    webJson("POST", "/api/keepawake/start", {
      duration_secs: a.durationSecs,
      display: a.display,
    }),
  keepawake_stop: () => webJson("POST", "/api/keepawake/stop"),

  // ── themes ── REST wraps the id list in an envelope; Tauri returns it bare — unwrap.
  list_themes: async () => {
    const resp = await webJson<{ themes?: string[] }>("GET", "/api/themes");
    return resp.themes ?? [];
  },
  delete_custom_theme: (a) =>
    webJson("DELETE", `/api/themes/${enc(s(a, "themeId"))}`),

  fs_list_dir: (a) =>
    webJson(
      "GET",
      `/api/fs/list-dir?path=${enc(s(a, "path"))}&show_hidden=${a.showHidden ? "true" : "false"}`,
    ),
  fs_mkdir: (a) =>
    webJson("POST", "/api/fs/mkdir", { parent: a.parent, name: a.name }),
  list_agent_providers: () => webJson("GET", "/api/agents/providers"),
  get_remote_workers: () => webJson("GET", "/api/remote-workers"),
  save_session: () => webJson("POST", "/api/session/save"),
  restart_general_agent: () => webJson("POST", "/api/agents/general/restart"),
  restart_general_agent_tab: (a) =>
    webJson("POST", `/api/agents/general/${enc(s(a, "tabId") ?? s(a, "tab_id"))}/restart`),
  restart_agent_tab: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId") ?? s(a, "tab_id"))}/agent/restart`),
  complete_general_agent_first_run: (a) =>
    webJson("POST", "/api/agents/general/complete-first-run", {
      provider_id: s(a, "providerId") ?? s(a, "provider_id"),
    }),
  save_temp_file: async (a) => {
    const resp = await webJson<{ path?: string }>("POST", "/api/attachments/temp-file", {
      data: a.data,
      extension: a.extension,
      name: a.name,
    });
    return resp.path ?? "";
  },
  copy_image_to_clipboard: (a) =>
    webJson("POST", "/api/clipboard/copy-image", { path: a.path }),
  save_clipboard_image: async (a) => {
    const resp = await webJson<{ path?: string }>(
      "POST",
      "/api/attachments/clipboard-image",
      { bytes: a.bytes, ext: a.ext },
    );
    return resp.path ?? "";
  },
  save_clipboard_file: async (a) => {
    const resp = await webJson<{ path?: string }>(
      "POST",
      "/api/attachments/clipboard-file",
      { bytes: a.bytes, filename: a.filename },
    );
    return resp.path ?? "";
  },
  install_provider_in_tab: (a) =>
    webJson(
      "POST",
      `/api/agents/providers/${enc(s(a, "providerId"))}/install-in-tab`,
    ),
  restart_app: () => webJson("POST", "/api/app/restart"),
  mux_link_pty: (a) =>
    webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/link-pty`, {
      pty_id: a.ptyId,
    }),
  write_pty_launch_command: (a) =>
    webJson("POST", `/api/pty/${enc(s(a, "ptyId"))}/launch-command`, {
      command: a.command,
    }),
  usage_latest: (a) => {
    const accountId = a.accountId ?? a.account_id;
    return webJson(
      "GET",
      `/api/usage/latest?provider=${enc(s(a, "provider"))}${accountId ? `&account_id=${enc(String(accountId))}` : ""}`,
    );
  },
  usage_history: (a) => {
    const accountId = a.accountId ?? a.account_id;
    const limit = a.limit == null ? "" : `&limit=${enc(String(a.limit))}`;
    return webJson(
      "GET",
      `/api/usage/history?provider=${enc(s(a, "provider"))}&since=${enc(String(a.since))}${limit}${accountId ? `&account_id=${enc(String(accountId))}` : ""}`,
    );
  },
  usage_projection: (a) => {
    const accountId = a.accountId ?? a.account_id;
    return webJson(
      "GET",
      `/api/usage/projection?provider=${enc(s(a, "provider"))}${accountId ? `&account_id=${enc(String(accountId))}` : ""}`,
    );
  },
  usage_refresh: (a) =>
    webJson("POST", "/api/usage/refresh", {
      provider: s(a, "provider"),
      account_id: a.accountId ?? a.account_id ?? null,
    }),
};

/** Browser-meaningless commands → canned results, no network. */
const CANNED: Record<string, unknown> = {
  get_windows_build_number: 19041,
  report_webgl_status: null,
  get_default_cwd: "",
  get_autostart_enabled: false,
  set_autostart_enabled: null,
  check_cli_installed: true,
  suggest_install_pwsh: null,
  get_shell_diagnostics: {},
  get_themes_dir: "",
  get_config_path: "",
  mux_update_tab_cwd: null,
  open_folder: null,
  // Closing a browser tab / unmounting a web terminal must never kill the shared server-side PTY other clients may be attached to.
  kill_pty: null,
};

export class WebUnsupportedError extends Error {
  readonly code = "WEB_UNSUPPORTED" as const;

  constructor(readonly command: string) {
    super(`[web] command not supported in browser mode: ${command}`);
    this.name = "WebUnsupportedError";
  }
}

export type WebCapability =
  | { supported: true }
  | { supported: false; reason: string };

/** Whether a Tauri IPC command is reachable from the web mirror. */
export function webCapability(cmd: string): WebCapability {
  if (!IS_WEB) return { supported: true };
  if (cmd in WEB_SPECIAL || cmd === "open_url") return { supported: true };
  if (cmd in HTTP_MAP || cmd in CANNED) return { supported: true };
  const reason = WEB_UNSUPPORTED[cmd];
  if (reason) return { supported: false, reason };
  return { supported: false, reason: `unmapped command: ${cmd}` };
}

export function isWebUnsupportedError(err: unknown): err is WebUnsupportedError {
  return err instanceof WebUnsupportedError;
}

async function invokeRaw<T>(cmd: string, args?: Args): Promise<T> {
  if (!IS_WEB) {
    return tauriInvoke<T>(cmd, args);
  }
  if (cmd === "open_url") {
    window.open(String(args?.url ?? ""), "_blank");
    return undefined as T;
  }
  const mapper = HTTP_MAP[cmd];
  if (mapper) {
    return (await mapper(args ?? {})) as T;
  }
  if (cmd in CANNED) {
    return CANNED[cmd] as T;
  }
  if (cmd in WEB_UNSUPPORTED) {
    throw new WebUnsupportedError(cmd);
  }
  const msg = `[web] command not available in browser mode: ${cmd}`;
  console.warn(msg, args);
  throw new Error(msg);
}

const IPC_SLOW_MS = 300;
type IpcSample = { cmd: string; ms: number; at: number };
type IpcPerf = {
  invokes: number;
  invokeMs: number;
  events: number;
  byCommand: Record<string, number>;
  byEvent: Record<string, number>;
};

declare global {
  interface Window {
    __ipcPerf?: IpcPerf;
  }
}

// Diagnostic: report any main-thread block (render / term.write / sync JS) >300ms.
if (typeof PerformanceObserver !== "undefined") {
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.duration >= IPC_SLOW_MS) {
          // eslint-disable-next-line no-console
          console.warn(`[LONGTASK] main thread blocked ${e.duration.toFixed(0)}ms`);
          const w = window as unknown as { __ctPerf?: IpcSample[] };
          (w.__ctPerf ??= []).push({ cmd: "LONGTASK", ms: e.duration, at: Date.now() });
        }
      }
    }).observe({ entryTypes: ["longtask"] });
  } catch { /* longtask unsupported */ }
}

export async function invoke<T>(cmd: string, args?: Args): Promise<T> {
  const t0 = performance.now();
  try {
    return invokeForTest
      ? await invokeForTest<T>(cmd, args)
      : await invokeRaw<T>(cmd, args);
  } catch (err) {
    if (isCommandNotFoundError(err)) {
      checkBuildMismatch().catch(() => {});
    }
    throw err;
  } finally {
    const perf = typeof window !== "undefined" ? window.__ipcPerf : undefined;
    if (perf) {
      perf.invokes += 1;
      perf.invokeMs += performance.now() - t0;
      perf.byCommand[cmd] = (perf.byCommand[cmd] ?? 0) + 1;
    }
    if (!invokeForTest && typeof window !== "undefined") {
      const ms = performance.now() - t0;
      if (ms >= IPC_SLOW_MS) {
        // eslint-disable-next-line no-console
        console.warn(`[SLOW-IPC] ${cmd} ${ms.toFixed(0)}ms`, args);
        const w = window as unknown as { __ctPerf?: IpcSample[] };
        (w.__ctPerf ??= []).push({ cmd, ms, at: Date.now() });
        if (w.__ctPerf.length > 200) w.__ctPerf.shift();
      }
    }
  }
}

function isCommandNotFoundError(err: unknown): boolean {
  const msg = String(err);
  return /command.*not found/i.test(msg);
}

/** Reopen a recently-closed tab in a workspace. `index` selects the ring
 *  entry (0 = most recent, the default). Works in both Tauri and web modes. */
export function reopenClosedTab(workspaceId: string, index = 0): Promise<TabInfo> {
  return invoke<TabInfo>("mux_reopen_closed_tab", { workspaceId, index });
}

/** Reopen a whole closed group (single / cascade subtree / workspace) by id.
 *  Recreates the workspace if needed and restores every tab, relinking the
 *  orchestration hierarchy. Returns workspace + tab ids of the activated tab. */
export function reopenClosedGroup(groupId: string): Promise<{ workspace_id: string; tab_id: string }> {
  return invoke("mux_reopen_closed_group", { groupId });
}

/** Reopen a tab from the orphaned ring (its workspace was deleted). Recreates
 *  the workspace automatically. Returns workspace + tab ids. */
export function restoreOrphanedTab(tabId: string): Promise<{ workspace_id: string; tab_id: string }> {
  return invoke("mux_restore_orphaned_tab", { tabId });
}

export async function listen<T>(
  event: string,
  handler: (e: { payload: T }) => void,
): Promise<UnlistenFn> {
  const counted = (e: { payload: T }) => {
    const perf = typeof window !== "undefined" ? window.__ipcPerf : undefined;
    if (perf) {
      perf.events += 1;
      perf.byEvent[event] = (perf.byEvent[event] ?? 0) + 1;
    }
    if (IS_WEB && event === "config-changed" && e.payload && typeof e.payload === "object") {
      const config = e.payload as Record<string, unknown>;
      const api = config.api as Record<string, unknown> | undefined;
      if (api) api.token = currentToken();
    }
    handler(e);
  };
  if (listenForTest) return listenForTest(event, counted);
  if (!IS_WEB) {
    return tauriListen<T>(event, counted);
  }
  return webListen(event, counted as (e: { payload: unknown }) => void);
}
