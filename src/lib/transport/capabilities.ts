import { TransportError, type TransportClient } from "../transportClient";
export type CapabilityArgs = Record<string, unknown>;
export type CapabilityMapper = (args: CapabilityArgs) => Promise<unknown>;
export type CapabilitiesClient = Pick<TransportClient, "json" | "localJson" | "currentToken">;
const enc = encodeURIComponent;
/** A refused link answers with a typed outcome body; only that body is turned back into a result. */
async function linkOutcome(request: Promise<unknown>): Promise<unknown> {
  try {
    return await request;
  } catch (error) {
    if (!(error instanceof TransportError)) throw error;
    let body: unknown = null;
    try {
      body = JSON.parse(error.body);
    } catch {
      throw error;
    }
    if (typeof body === "object" && body !== null && (body as { ok?: unknown }).ok === false) return body;
    throw error;
  }
}
const s = (args: CapabilityArgs, key: string): string => String(args[key] ?? "");
function previewTab(args: CapabilityArgs): string {
  const tabId = s(args, "tabId");
  if (!tabId) throw new Error("Local previews require a tab identity");
  return enc(tabId);
}
export function createCapabilitiesTransport(client: CapabilitiesClient) {
  const webJson = client.json.bind(client);
  const localJson = client.localJson.bind(client);
  const currentToken = client.currentToken.bind(client);
  return {
    account_mesh_state: () => localJson("GET", "/api/account/mesh"),
    account_mesh_enroll: () => localJson("POST", "/api/account/mesh/enroll"),
    account_mesh_list: () => localJson("POST", "/api/account/mesh/list"),
    account_mesh_rename_self: (a) =>
      localJson("POST", "/api/account/mesh/rename", {
        expected_metadata_revision: a.expectedMetadataRevision,
        display_name: a.displayName,
      }),
    account_mesh_session_revoke: (a) => localJson("POST", "/api/account/mesh/session-revoke", { installation_id: a.installationId ?? null }),
    account_mesh_revoke: (a) => localJson("POST", "/api/account/mesh/revoke", { target: a.target }),
    account_mesh_acknowledge_limit: (a) =>
      localJson("POST", "/api/account/mesh/limit-notice/ack", { request_id: s(a, "requestId") }),
    answer_notification: (a) =>
      webJson("POST", `/api/notifications/${enc(s(a, "id"))}/answer`, { label: s(a, "label") }),
    restore_provider_integration: (a) =>
      webJson("POST", `/api/setup/hooks/restore?provider=${enc(s(a, "provider_id"))}`),
    check_provider_rule: (a) =>
      webJson("GET", `/api/setup/rules?provider=${enc(s(a, "provider_id"))}`),
    install_provider_rule: (a) =>
      webJson("POST", `/api/setup/rules/install?provider=${enc(s(a, "provider_id"))}`),
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
    check_local_preview_files: (a) => webJson("POST", `/api/tabs/${previewTab(a)}/preview/check`, { paths: a.paths }),
    check_provider_integration: (a) =>
      webJson("GET", `/api/setup/hooks?provider=${enc(s(a, "provider_id"))}`),
    clear_notifications: (a) =>
      a.id
        ? webJson("DELETE", `/api/notifications/${enc(s(a, "id"))}`)
        : webJson("POST", "/api/notifications/clear"),
    delayed_send_create: (a) =>
      webJson("POST", `/api/tabs/${enc(s(a, "tabId"))}/delayed-messages`, {
        text: a.text,
        scheduledAtMs: a.scheduledAtMs,
      }),
    delayed_send_delete: (a) =>
      webJson("DELETE", `/api/delayed-messages/${enc(s(a, "id"))}`),
    delayed_send_list: (a) =>
      webJson("GET", `/api/tabs/${enc(s(a, "tabId"))}/delayed-messages`),
    system_heartbeat_list: (a) =>
      webJson("GET", `/api/tabs/${enc(s(a, "tabId"))}/system-heartbeats`),
    delayed_send_send_now: (a) =>
      webJson("POST", `/api/delayed-messages/${enc(s(a, "id"))}/send-now`),
    delayed_send_update: (a) =>
      webJson("PATCH", `/api/delayed-messages/${enc(s(a, "id"))}`, {
        text: a.text,
        scheduledAtMs: a.scheduledAtMs,
      }),
    delete_custom_theme: (a) =>
      webJson("DELETE", `/api/themes/${enc(s(a, "themeId"))}`),
    disable_web_access: () => webJson("POST", "/api/network/serve/disable"),
    enable_web_access: (a) =>
      webJson("POST", "/api/network/serve/enable", { https_port: a.https_port }),
    fs_list_dir: (a) =>
      webJson(
        "GET",
        `/api/fs/list-dir?path=${enc(s(a, "path"))}&show_hidden=${a.showHidden ? "true" : "false"}`,
      ),
    fs_mkdir: (a) =>
      webJson("POST", "/api/fs/mkdir", { parent: a.parent, name: a.name }),
    get_config: async () => {
      const config = await webJson<Record<string, unknown>>("GET", "/api/config");
      const api = config.api as Record<string, unknown> | undefined;
      if (api) api.token = currentToken();
      return config;
    },
    get_network_info: () => webJson("GET", "/api/network"),
    get_serve_status: () => webJson("GET", "/api/network/serve/status"),
    install_provider_in_tab: (a) =>
      webJson(
        "POST",
        `/api/agents/providers/${enc(s(a, "providerId"))}/install-in-tab`,
      ),
    keepawake_start: (a) =>
      webJson("POST", "/api/keepawake/start", {
        duration_secs: a.durationSecs,
        display: a.display,
      }),
    keepawake_status: () => webJson("GET", "/api/keepawake"),
    keepawake_stop: () => webJson("POST", "/api/keepawake/stop"),

    list_agent_providers: () => webJson("GET", "/api/agents/providers"),
    list_notifications: () => webJson("GET", "/api/notifications"),
    list_themes: async () => {
      const resp = await webJson<{ themes?: string[] }>("GET", "/api/themes");
      return resp.themes ?? [];
    },
    mark_notification_read: (a) =>
      webJson("POST", `/api/notifications/${enc(s(a, "id"))}/read`),
    account_mesh_link: (a) =>
      linkOutcome(
        localJson("POST", "/api/mesh/link", {
          installation_id: s(a, "installationId"),
          request_id: a.requestId,
        }),
      ),
    mesh_disconnect: (a) =>
      webJson("POST", `/api/mesh/peers/disconnect/${enc(s(a, "instanceId"))}`),
    mesh_exit: () => webJson("POST", "/api/mesh/exit"),
    mesh_get_status: () => webJson("GET", "/api/mesh/status"),
    mesh_inventory_list: () => webJson("GET", "/api/mesh/inventory"),
    mesh_list_peers: () => webJson("GET", "/api/mesh/peers"),
    mesh_peer_cron_action: (a) =>
      webJson(
        "POST",
        `/api/mesh/peers/${enc(s(a, "instanceId"))}/crons/${enc(s(a, "id"))}/${enc(s(a, "action"))}`,
      ),
    mesh_peer_cron_delete: (a) =>
      webJson(
        "DELETE",
        `/api/mesh/peers/${enc(s(a, "instanceId"))}/crons/${enc(s(a, "id"))}`,
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
    mesh_peer_cron_update: (a) =>
      webJson(
        "PUT",
        `/api/mesh/peers/${enc(s(a, "instanceId"))}/crons/${enc(s(a, "id"))}`,
        a.patch,
      ),
    mesh_peer_service_delete: (a) =>
      webJson(
        "DELETE",
        `/api/mesh/peers/${enc(s(a, "instanceId"))}/services/${enc(s(a, "id"))}`,
      ),
    mesh_peer_services_list: (a) =>
      webJson("GET", `/api/mesh/peers/${enc(s(a, "instanceId"))}/services`),
    mesh_set_access: (a) =>
      webJson("POST", `/api/mesh/peers/${enc(s(a, "instanceId"))}/access`, {
        access_level: a.accessLevel,
      }),
    mesh_set_enabled: (a) => webJson("POST", "/api/mesh/enabled", { enabled: a.enabled }),
    mesh_transfer_attachment: (a) =>
      localJson("POST", "/api/mesh/attachments/transfer", {
        ordinary_tab_id: s(a, "ordinaryTabId"),
        peer_id: s(a, "peerId"),
        path: s(a, "path"),
        kind: s(a, "kind"),
      }),
    model_provider_manage: (a) => localJson(
      "POST",
      a.providerId ? `/api/model-providers?provider=${encodeURIComponent(String(a.providerId))}` : "/api/model-providers",
      a.request,
    ),
    open_chat_backend_tab: (a) =>
      webJson("POST", "/api/plugins/chatbackend/open", {
        pluginId: s(a, "pluginId"),
        workspaceId: a.workspaceId,
        config: a.config,
        systemPrompt: a.systemPrompt,
        preset: a.preset,
        model: a.model,
      }),
    plugin_bubble: (a) => webJson("POST", "/api/plugins/bubble", { pluginId: a.pluginId, ctx: a.ctx }, { signal: a.signal as AbortSignal | undefined }),
    plugin_glance: (a) => webJson("POST", "/api/plugins/glance", a),
    plugin_manager_action: (a) => webJson("POST", "/api/plugins/action", a.action),
    plugin_manager_snapshot: (a) => webJson("GET", "/api/plugins/snapshot", undefined, { signal: a.signal as AbortSignal | undefined }),
    plugin_tab_menu: () => webJson("GET", "/api/plugins/tab-menu"),
    plugin_tab_menu_action: (a) => webJson("POST", "/api/plugins/tab-menu/action", a),
    plugin_remote_task_call: (a) => client.localJson("POST", "/api/plugins/remote-task-call", a),
    plugin_recording_suggestion: (a) => client.localJson("POST", "/api/plugins/recording-suggestion", a),
    plugin_view_call: (a) => webJson("POST", "/api/plugins/view-call", a),
    provider_cli_status: (a) => webJson("GET", `/api/agents/providers/${enc(s(a, "providerId"))}/cli`),
    push_channel_roster: () => webJson("GET", "/api/notifications/channels"),
    read_html_preview_file: async (a) => {
      const file = await webJson<{ data: string }>("POST", `/api/tabs/${previewTab(a)}/preview/read`, { path: a.path, kind: "page" });
      return file.data;
    },
    read_local_preview_file: (a) => webJson("POST", `/api/tabs/${previewTab(a)}/preview/read`, { path: a.path, kind: a.kind }),
    read_workspace_essence_maps: (a) => {
      const paths = Array.isArray(a.projectPaths) ? a.projectPaths : [];
      return webJson("POST", "/api/essence-maps", { path: paths });
    },
    read_workspace_essence_source: (a) =>
      webJson(
        "GET",
        `/api/essence-source?project=${enc(s(a, "projectPath"))}&path=${enc(s(a, "sourcePath"))}`,
      ),
    refresh_provider_models: (a) =>
      webJson("POST", "/api/agents/providers/refresh", { provider_id: String(a.providerId ?? a.provider_id ?? "") }),
    repo_watch_subscribe: (a) => webJson("POST", "/api/repo-watch/subscribe", a),
    repo_watch_unsubscribe: (a) => webJson("POST", "/api/repo-watch/unsubscribe", a),
    save_clipboard_file: async (a) => {
      const resp = await webJson<{ path?: string }>(
        "POST",
        "/api/attachments/clipboard-file",
        { bytes: a.bytes, filename: a.filename },
      );
      return resp.path ?? "";
    },
    save_clipboard_image: async (a) => {
      const resp = await webJson<{ path?: string }>(
        "POST",
        "/api/attachments/clipboard-image",
        { bytes: a.bytes, ext: a.ext },
      );
      return resp.path ?? "";
    },
    services_create: (a) => webJson("POST", "/api/services", a.input),
    services_delete: (a) => webJson("DELETE", `/api/services/${enc(s(a, "id"))}`),
    services_list: () => webJson("GET", "/api/services"),
    services_regenerate_token: (a) =>
      webJson("POST", `/api/services/${enc(s(a, "id"))}/regenerate-token`),
    services_smart_add_inspect: (a) =>
      webJson("POST", "/api/services/smart-add-inspect", { url: a.url }),
    services_update: (a) =>
      webJson("PATCH", `/api/services/${enc(s(a, "id"))}`, a.patch),
    session_transcript: (a) =>
      webJson(
        "GET",
        `/api/sessions/transcript?provider=${enc(s(a, "provider"))}&cwd=${enc(s(a, "cwd"))}&session_id=${enc(s(a, "sessionId"))}`,
      ),
    transcribe_voice_attachment: async (a) => {
      const resp = await webJson<{ line?: string }>("POST", "/api/transcribe-voice", {
        path: s(a, "path"),
      });
      return resp.line ?? "";
    },
    update_config: (a) => webJson("PATCH", "/api/config", a.config),
    usage_history: (a) => {
      const accountId = a.accountId ?? a.account_id;
      const limit = a.limit == null ? "" : `&limit=${enc(String(a.limit))}`;
      return webJson(
        "GET",
        `/api/usage/history?provider=${enc(s(a, "provider"))}&since=${enc(String(a.since))}${limit}${accountId ? `&account_id=${enc(String(accountId))}` : ""}`,
      );
    },
    usage_latest: (a) => {
      const accountId = a.accountId ?? a.account_id;
      return webJson(
        "GET",
        `/api/usage/latest?provider=${enc(s(a, "provider"))}${accountId ? `&account_id=${enc(String(accountId))}` : ""}`,
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
    install_provider_integration: (a) => {
      if (a.consent !== true) return Promise.reject(new Error("Integration repair requires consent"));
      return webJson("POST", `/api/setup/hooks/install?provider=${enc(s(a, "provider_id"))}`, { consent: true, provider: s(a, "provider_id") });
    },
    uninstall_provider_integration: (a) => webJson("POST", `/api/setup/hooks/uninstall?provider=${enc(s(a, "provider_id"))}`, { provider: s(a, "provider_id") }),
    provider_model_usage: (a) => webJson("GET", `/api/agents/providers/${enc(s(a, "providerId"))}/model-usage`),
    get_default_cwd: () => webJson("GET", "/api/system/default-cwd"),
    get_shell_diagnostics: () => webJson("GET", "/api/system/shell-diagnostics"),
    suggest_install_pwsh: () => webJson("POST", "/api/system/install-pwsh"),
    get_themes_dir: () => webJson("GET", "/api/themes/location"),
    attachment_path_kinds: (a) => webJson("POST", "/api/attachments/path-kinds", { paths: a.paths }),
    report_webgl_status: (a) => webJson("POST", "/api/diagnostics/webgl", { state: a.state, detail: a.detail }),
  } satisfies Record<string, CapabilityMapper>;
}
export function invokeCapability<T>(registry: Record<string, CapabilityMapper>, command: string, args: CapabilityArgs = {}): Promise<T> {
  if (!Object.prototype.hasOwnProperty.call(registry, command)) return Promise.reject(new Error(`No audited HTTP route for command: ${command}`));
  return registry[command](args) as Promise<T>;
}
