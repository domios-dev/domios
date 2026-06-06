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
import { IS_WEB, webJson } from "./webApi";
import { webListen } from "./webEvents";

export type UnlistenFn = TauriUnlistenFn;
export { IS_WEB };

type Args = Record<string, unknown>;
type Mapper = (args: Args) => Promise<unknown>;

const enc = encodeURIComponent;
const s = (args: Args, key: string): string => String(args[key] ?? "");

/** Commands with a meaningful HTTP equivalent. */
const HTTP_MAP: Record<string, Mapper> = {
  // ── config ──
  get_config: () => webJson("GET", "/api/config"),
  update_config: (a) => webJson("POST", "/api/config", a.config),

  // ── multiplexer ──
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
  mux_create_tab: (a) =>
    webJson("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs`, a),
  mux_set_active_tab: (a) =>
    webJson(
      "POST",
      `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/focus`,
    ),
  mux_close_tab: (a) =>
    webJson(
      "POST",
      `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/close`,
    ),
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
  mux_set_focused_pane: (a) =>
    webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/focus`),
  mux_close_pane: (a) => webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/close`),
  mux_respond_prompt: (a) =>
    webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/prompt/respond`, a),
  mux_set_chat_view: (a) =>
    webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/chat-view`, {
      enabled: a.chatView ?? a.enabled,
    }),

  // ── panes (read + input) ──
  get_pane_text: (a) =>
    webJson("GET", `/api/panes/${enc(s(a, "paneId"))}/text?formatted=${a.formatted ? "true" : "false"}`),
  get_pane_output: (a) => webJson("GET", `/api/panes/${enc(s(a, "paneId"))}/output`),

  // ── chat ──
  chat_start: (a) => webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/chat/start`, a),
  chat_stop: (a) => webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/chat/stop`),
  chat_backfill: (a) =>
    webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/chat/backfill`, a),
  chat_send_text: (a) =>
    webJson("POST", `/api/panes/${enc(s(a, "paneId"))}/send`, {
      text: a.text,
      enter: true,
    }),

  // ── mesh ──
  mesh_get_status: () => webJson("GET", "/api/mesh/status"),
  mesh_list_peers: () => webJson("GET", "/api/mesh/peers"),
  mesh_peer_services_list: (a) =>
    webJson("GET", `/api/mesh/peers/${enc(s(a, "instanceId"))}/services`),
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
  mesh_start_broadcast: () => webJson("POST", "/api/mesh/broadcast/start"),
  mesh_stop_broadcast: () => webJson("POST", "/api/mesh/broadcast/stop"),

  // ── services / activity / crons ──
  services_list: () => webJson("GET", "/api/services"),
  services_create: (a) => webJson("POST", "/api/services", a.params ?? a),
  services_update: (a) =>
    webJson("PATCH", `/api/services/${enc(s(a, "id"))}`, a.params ?? a),
  services_delete: (a) => webJson("DELETE", `/api/services/${enc(s(a, "id"))}`),
  services_regenerate_token: (a) =>
    webJson("POST", `/api/services/${enc(s(a, "id"))}/regenerate-token`),
  activity_list: () => webJson("GET", "/api/activity"),
  cron_delete: (a) => webJson("DELETE", `/api/crons/${enc(s(a, "id"))}`),
  cron_trigger: (a) => webJson("POST", `/api/crons/${enc(s(a, "id"))}/trigger`),
  cron_toggle: (a) =>
    webJson(
      "POST",
      `/api/crons/${enc(s(a, "id"))}/${a.enabled ? "enable" : "disable"}`,
    ),
  cron_update: (a) => webJson("PATCH", `/api/crons/${enc(s(a, "id"))}`, a.params ?? a),

  // ── PTY (browser terminal I/O loop) ──
  read_pty: (a) => webJson("GET", `/api/pty/${enc(s(a, "ptyId"))}/read`),
  write_pty_string: (a) =>
    webJson("POST", `/api/pty/${enc(s(a, "ptyId"))}/write`, { data: a.data }),
  resize_pty: (a) =>
    webJson("POST", `/api/pty/${enc(s(a, "ptyId"))}/resize`, {
      cols: a.cols,
      rows: a.rows,
    }),

  // ── misc with real endpoints ──
  list_agent_providers: () => webJson("GET", "/api/agents/providers"),
  save_session: () => webJson("POST", "/api/session/save"),
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
  keepawake_status: { active: false },
  keepawake_start: null,
  keepawake_stop: null,
  mux_update_pane_cwd: null,
  usage_refresh: null,
  open_folder: null,
  // Closing a browser tab / unmounting a web terminal must never kill the
  // shared server-side PTY other clients may be attached to.
  kill_pty: null,
};

export async function invoke<T>(cmd: string, args?: Args): Promise<T> {
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
  const msg = `[web] command not available in browser mode: ${cmd}`;
  console.warn(msg, args);
  throw new Error(msg);
}

export async function listen<T>(
  event: string,
  handler: (e: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (!IS_WEB) {
    return tauriListen<T>(event, handler);
  }
  return webListen(event, handler as (e: { payload: unknown }) => void);
}
