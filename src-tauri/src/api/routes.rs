use axum::{
    http::{HeaderMap, StatusCode},
    middleware,
    routing::{delete, get, post},
    Router,
};
use tower_http::{catch_panic::CatchPanicLayer, compression::CompressionLayer, cors::CorsLayer};

use super::{
    agent_routes, bridge_routes, config_routes, cron_routes, debug_routes, delayed_send_routes,
    fs_routes, health_routes, hook_routes, logs_routes, mem_routes, mesh_routes, network_routes,
    notification_routes, pane_chat_routes, pane_routes, pasteboard_routes, services_routes,
    session_routes, theme_routes, updater_routes, usage_routes, web_routes, webhook_routes,
    workspace_routes,
};

#[derive(Clone)]
pub struct AppState {
    pub svc: crate::service::ServiceState,
    pub cron: crate::CronState,
    pub cron_wake: crate::CronWake,
    pub api_port: u16,
    pub update_state: crate::UpdateStateHandle,
    pub config: crate::config::ConfigState,
    pub mesh: crate::MeshStateHandle,
    pub keepawake: crate::KeepAwakeStateHandle,
    pub resize_owner: crate::ResizeOwnerState,
    /// Some only in debug builds started with CODETERM_TEST_CLOCK=1 —
    /// powers /api/debug/clock time travel for the cron e2e suite.
    pub test_clock: Option<std::sync::Arc<crate::cron::clock::MockClock>>,
    /// System event bus — feeds the web mirror's SSE stream.
    pub events: crate::api::events::EventBroadcasterState,
    /// Directory holding the built React bundle, served at /web.
    pub web_dist: Option<std::path::PathBuf>,
    /// Short-lived web tokens for PWA/mirror auth. Maps token → creation instant;
    /// avoids exposing the permanent master API token outside the daemon.
    /// Tokens expire after WEB_TOKEN_TTL.
    pub web_tokens:
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>>,
}

/// Lifetime of web tokens minted by mesh_web_token. After this period the token
/// is no longer accepted by the auth middleware and the browser must re-fetch.
pub(crate) const WEB_TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

pub fn create_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/api/health", get(health_routes::health))
        .route("/api/identify", get(health_routes::identify))
        .route("/api/fs/list-dir", get(fs_routes::list_dir))
        .route("/api/fs/mkdir", post(fs_routes::mkdir))
        .route(
            "/api/workspaces",
            get(workspace_routes::list_workspaces).post(workspace_routes::create_workspace),
        )
        .route(
            "/api/workspaces/{id}/focus",
            post(workspace_routes::focus_workspace),
        )
        .route(
            "/api/workspaces/{id}/rename",
            post(workspace_routes::rename_workspace),
        )
        .route(
            "/api/workspaces/{id}/close",
            post(workspace_routes::close_workspace),
        )
        .route(
            "/api/workspaces/{id}/recently-closed",
            get(workspace_routes::list_recently_closed),
        )
        .route(
            "/api/workspaces/{id}/reopen-closed",
            post(workspace_routes::reopen_closed_pane),
        )
        .route(
            "/api/closed-groups/{group_id}/reopen",
            post(workspace_routes::reopen_closed_group),
        )
        .route(
            "/api/workspaces/{id}/tabs",
            post(workspace_routes::create_tab),
        )
        .route(
            "/api/workspaces/{id}/tabs/{tab_id}/rename",
            post(workspace_routes::rename_tab),
        )
        .route(
            "/api/workspaces/{id}/tabs/{tab_id}/move",
            post(workspace_routes::move_tab),
        )
        .route(
            "/api/panes",
            get(pane_routes::list_panes).post(pane_routes::create_shell_pane),
        )
        .route("/api/panes/{id}/send", post(pane_routes::send_text))
        .route("/api/panes/{id}/stop", post(pane_routes::stop_pane))
        .route("/api/panes/{id}/focus", post(pane_routes::focus_pane))
        .route("/api/panes/{id}/close", post(pane_routes::close_pane))
        .route("/api/panes/{id}/rename", post(pane_routes::rename_pane))
        .route("/api/panes/{id}/describe", post(pane_routes::describe_pane))
        .route("/api/panes/{id}/output", get(pane_routes::get_pane_output))
        .route(
            "/api/panes/{id}/output/stream",
            get(pane_routes::stream_pane_output),
        )
        .route(
            "/api/agent-runs/{pane_id}/log",
            get(pane_routes::agent_run_log),
        )
        .route("/api/panes/{id}/text", get(pane_routes::get_pane_text))
        .route(
            "/api/panes/{id}/shell-commands",
            get(pane_routes::shell_command_snapshot),
        )
        .route("/api/panes/{id}/usage", get(pane_routes::get_pane_usage))
        .route("/api/panes/{id}/exec", post(pane_routes::exec_in_pane))
        .route(
            "/api/panes/{id}/capture/start",
            post(pane_routes::start_capture),
        )
        .route(
            "/api/panes/{id}/capture/stop",
            post(pane_routes::stop_capture),
        )
        .route("/api/panes/{id}/resize", post(pane_routes::resize_pane))
        .route(
            "/api/panes/{id}/resize-owner",
            post(web_routes::claim_resize_owner),
        )
        .route("/api/panes/{id}/chat", get(pane_chat_routes::snapshot))
        .route("/api/panes/{id}/chat/start", post(pane_chat_routes::start))
        .route("/api/panes/{id}/chat/stop", post(pane_chat_routes::stop))
        .route(
            "/api/panes/{id}/chat/backfill",
            post(pane_chat_routes::backfill),
        )
        .route(
            "/api/panes/{id}/delayed-messages",
            get(delayed_send_routes::list).post(delayed_send_routes::create),
        )
        .route(
            "/api/delayed-messages/{id}",
            axum::routing::patch(delayed_send_routes::update).delete(delayed_send_routes::delete),
        )
        .route(
            "/api/delayed-messages/{id}/send-now",
            post(delayed_send_routes::send_now),
        )
        .route("/api/serve-file", get(notification_routes::serve_file))
        .route(
            "/api/notifications",
            get(notification_routes::list_notifications)
                .post(notification_routes::create_notification),
        )
        .route(
            "/api/notifications/clear",
            post(notification_routes::clear_notifications),
        )
        .route(
            "/api/webhooks",
            get(webhook_routes::list_webhooks).post(webhook_routes::register_webhook),
        )
        .route(
            "/api/webhooks/{id}",
            axum::routing::delete(webhook_routes::unregister_webhook),
        )
        .route("/api/network", get(network_routes::network_info))
        .route(
            "/api/network/serve/status",
            get(network_routes::serve_status_handler),
        )
        .route(
            "/api/network/serve/enable",
            post(network_routes::serve_enable_handler),
        )
        .route(
            "/api/network/serve/disable",
            post(network_routes::serve_disable_handler),
        )
        .route(
            "/api/keepawake",
            get(network_routes::keepawake_status_handler),
        )
        .route(
            "/api/keepawake/start",
            post(network_routes::keepawake_start_handler),
        )
        .route(
            "/api/keepawake/stop",
            post(network_routes::keepawake_stop_handler),
        )
        .route(
            "/api/themes",
            get(theme_routes::list_themes_api).post(theme_routes::save_theme_api),
        )
        .route(
            "/api/themes/{id}",
            axum::routing::delete(theme_routes::delete_theme_api),
        )
        .route("/api/themes/apply", post(theme_routes::apply_theme_api))
        .route("/api/setup/rules", get(theme_routes::check_rules_api))
        .route(
            "/api/setup/rules/install",
            post(theme_routes::install_rules_api),
        )
        .route("/api/setup/hooks", get(theme_routes::check_hooks_api))
        .route(
            "/api/setup/hooks/install",
            post(theme_routes::install_hooks_api),
        )
        .route(
            "/api/setup/hooks/uninstall",
            post(theme_routes::uninstall_hooks_api),
        )
        .route(
            "/api/panes/{id}/role",
            post(agent_routes::set_pane_role).delete(agent_routes::clear_pane_role),
        )
        .route("/api/panes/{id}/status", get(agent_routes::get_pane_status))
        .route("/api/panes/{id}/report", post(agent_routes::submit_report))
        .route("/api/panes/{id}/reports", get(agent_routes::get_reports))
        .route(
            "/api/panes/{id}/can-spawn",
            post(agent_routes::set_can_spawn),
        )
        .route(
            "/api/panes/{id}/agent/restart",
            post(agent_routes::agent_restart_handler),
        )
        .route(
            "/api/panes/{id}/agent-type",
            post(agent_routes::set_agent_type),
        )
        .route(
            "/api/panes/{id}/prompt/respond",
            post(agent_routes::respond_prompt),
        )
        .route(
            "/api/panes/{id}/prompt/clear",
            post(agent_routes::clear_prompt),
        )
        .route(
            "/api/panes/{id}/hook/permission",
            post(hook_routes::hook_permission),
        )
        .route("/api/panes/{id}/hook/stop", post(hook_routes::hook_stop))
        .route(
            "/api/panes/{id}/hook/session_start",
            post(hook_routes::hook_session_start),
        )
        .route(
            "/api/agents/providers",
            get(agent_routes::list_agent_providers),
        )
        .route("/api/debug/clock", get(debug_routes::clock_status))
        .route(
            "/api/debug/clock/advance",
            post(debug_routes::clock_advance),
        )
        // Web mirror: IPC equivalents + the SSE event stream.
        .route("/api/mux/state", get(web_routes::mux_state))
        .route(
            "/api/workspaces/{id}/tabs/{tab_id}/focus",
            post(web_routes::tab_focus),
        )
        .route(
            "/api/workspaces/{id}/tabs/{tab_id}/close",
            post(web_routes::tab_close),
        )
        .route(
            "/api/panes/{id}/chat-view",
            post(web_routes::pane_chat_view),
        )
        .route(
            "/api/mesh/peers/{id}/services",
            get(web_routes::peer_services),
        )
        .route("/api/events/stream", get(web_routes::events_stream))
        .route("/api/transcribe-voice", post(web_routes::transcribe_voice))
        .route(
            "/api/panes/{id}/chat-raw/start",
            post(web_routes::chat_raw_start),
        )
        .route(
            "/api/panes/{id}/chat-raw/stop",
            post(web_routes::chat_raw_stop),
        )
        .route(
            "/api/panes/{id}/chat-raw/backfill",
            post(web_routes::chat_raw_backfill),
        )
        .route(
            "/api/sessions/transcript",
            get(web_routes::session_transcript),
        )
        .route(
            "/api/upload",
            post(web_routes::upload)
                // Voice notes and attached files: axum's 2MB default is too small; cap at 200MB so a runaway client can't fill the disk.
                .layer(axum::extract::DefaultBodyLimit::max(200 * 1024 * 1024)),
        )
        .route("/api/pty/{id}/read", get(web_routes::pty_read))
        .route("/api/pty/{id}/write", post(web_routes::pty_write))
        .route("/api/pty/{id}/resize", post(web_routes::pty_resize))
        .route("/api/session/save", post(session_routes::save_session_api))
        .route(
            "/api/providers",
            get(agent_routes::list_agent_providers_public),
        )
        .route(
            "/api/sessions/search",
            get(session_routes::search_sessions_api),
        )
        .route("/api/sessions/{id}", get(session_routes::get_session_api))
        .route(
            "/api/sessions/{id}/resume",
            post(session_routes::resume_session_api),
        )
        .route("/api/agents/spawn", post(agent_routes::spawn_agent))
        .route("/api/batch", post(agent_routes::batch_commands))
        .route(
            "/api/crons",
            get(cron_routes::cron_list).post(cron_routes::cron_create),
        )
        .route(
            "/api/crons/{id}",
            get(cron_routes::cron_get)
                .put(cron_routes::cron_update)
                .delete(cron_routes::cron_delete),
        )
        .route("/api/crons/{id}/enable", post(cron_routes::cron_enable))
        .route("/api/crons/{id}/disable", post(cron_routes::cron_disable))
        .route("/api/crons/{id}/trigger", post(cron_routes::cron_trigger))
        .route("/api/crons/{id}/cancel", post(cron_routes::cron_cancel))
        .route("/api/crons/{id}/history", get(cron_routes::cron_history))
        .route(
            "/api/services",
            get(services_routes::list_services).post(services_routes::create_service),
        )
        .route(
            "/api/services/{id}",
            get(services_routes::get_service)
                .put(services_routes::update_service)
                .delete(services_routes::delete_service),
        )
        .route(
            "/api/services/{id}/regenerate-token",
            post(services_routes::regenerate_token),
        )
        .route("/api/activity", get(services_routes::list_activity))
        .route(
            "/api/pasteboard",
            get(pasteboard_routes::paste_list).post(pasteboard_routes::paste_create),
        )
        .route(
            "/api/pasteboard/{id}",
            get(pasteboard_routes::paste_get).delete(pasteboard_routes::paste_delete),
        )
        .route(
            "/api/config",
            get(config_routes::get_config_api).patch(config_routes::patch_config_api),
        )
        .route("/api/update/check", get(updater_routes::check_update))
        .route("/api/update/status", get(updater_routes::update_status))
        .route("/api/update/install", post(updater_routes::install_update))
        .route(
            "/api/update/channel",
            get(updater_routes::get_channel).post(updater_routes::set_channel),
        )
        .route("/api/bridge/send", post(config_routes::bridge_send_api))
        .route("/api/bridge/state", get(bridge_routes::bridge_state))
        .route("/api/bridge/start", post(bridge_routes::bridge_start))
        .route("/api/bridge/stop", post(bridge_routes::bridge_stop))
        .route("/api/bridge/announce", post(bridge_routes::bridge_announce))
        .route("/api/mesh/ping", get(mesh_routes::mesh_ping))
        .route("/api/mesh/debug/sign", get(mesh_routes::mesh_debug_sign))
        .route("/api/mesh/status", get(mesh_routes::mesh_status))
        .route("/api/mesh/peers", get(mesh_routes::mesh_peers))
        .route("/api/mesh/web-token", get(mesh_routes::mesh_web_token))
        .route(
            "/api/mesh/peers/{id}/web-token",
            get(mesh_routes::mesh_peer_web_token),
        )
        .route("/api/mesh/discover", post(mesh_routes::mesh_discover))
        .route("/api/mesh/connect", post(mesh_routes::mesh_connect_peer))
        .route("/api/mesh/approve", post(mesh_routes::mesh_approve_peer))
        .route("/api/mesh/reject", post(mesh_routes::mesh_reject_peer))
        .route(
            "/api/mesh/disconnect",
            post(mesh_routes::mesh_disconnect_peer),
        )
        .route(
            "/api/mesh/pair/request",
            post(mesh_routes::mesh_pair_request),
        )
        .route("/api/mesh/pair/accept", post(mesh_routes::mesh_pair_accept))
        .route(
            "/api/mesh/pair/confirm",
            post(mesh_routes::mesh_pair_confirm),
        )
        .route("/api/mesh/pair/reject", post(mesh_routes::mesh_pair_reject))
        .route("/api/mesh/introduce", post(mesh_routes::mesh_introduce))
        .route("/api/mesh/sync", post(mesh_routes::mesh_sync))
        .route(
            "/api/mesh/broadcast/start",
            post(mesh_routes::mesh_start_broadcast),
        )
        .route(
            "/api/mesh/broadcast/stop",
            post(mesh_routes::mesh_stop_broadcast),
        )
        .route("/api/mesh/relay", post(mesh_routes::mesh_relay))
        .route(
            "/api/mesh/peers/{id}",
            axum::routing::delete(mesh_routes::mesh_unpair),
        )
        .route("/api/mesh/refresh", post(mesh_routes::mesh_refresh))
        .route(
            "/api/mesh/peers/add-manual",
            post(mesh_routes::mesh_add_manual),
        )
        .route(
            "/api/mesh/peers/approve/{id}",
            post(mesh_routes::mesh_approve_peer_by_id),
        )
        .route(
            "/api/mesh/peers/reject/{id}",
            post(mesh_routes::mesh_reject_peer_by_id),
        )
        .route(
            "/api/mesh/peers/disconnect/{id}",
            post(mesh_routes::mesh_disconnect_peer_by_id),
        )
        .route(
            "/api/mesh/announce",
            post(mesh_routes::mesh_announce_inbound),
        )
        .route(
            "/api/mesh/announce/all",
            post(mesh_routes::mesh_announce_all),
        )
        .route(
            "/api/mesh/peers/{id}/addr",
            post(mesh_routes::mesh_set_peer_addr),
        )
        .route("/api/mem/search", get(mem_routes::mem_search))
        .route("/api/mem/items", post(mem_routes::mem_save))
        .route("/api/mem/pinned", get(mem_routes::mem_pinned_list))
        .route(
            "/api/mem/items/{id}",
            get(mem_routes::mem_get)
                .delete(mem_routes::mem_delete)
                .patch(mem_routes::mem_update),
        )
        .route("/api/mem/items/{id}/history", get(mem_routes::mem_history))
        .route("/api/mem/taxonomy", get(mem_routes::mem_taxonomy))
        .route("/api/mem/tail", get(mem_routes::mem_tail))
        .route(
            "/api/mem/forget/{area}",
            delete(mem_routes::mem_forget_area),
        )
        .route("/api/mem/wake-up", get(mem_routes::mem_wake_up))
        .route("/api/mem/stats", get(mem_routes::mem_stats))
        .route("/api/mem/reembed", post(mem_routes::mem_reembed))
        .route(
            "/api/mem/graph/entities",
            post(mem_routes::mem_graph_create),
        )
        .route("/api/mem/graph/relate", post(mem_routes::mem_graph_relate))
        .route("/api/mem/graph/related", get(mem_routes::mem_graph_related))
        .route(
            "/api/mem/graph/invalidate",
            post(mem_routes::mem_graph_invalidate),
        )
        .route(
            "/api/mem/graph/timeline",
            get(mem_routes::mem_graph_timeline),
        )
        .route(
            "/api/mem/graph/traverse",
            get(mem_routes::mem_graph_traverse),
        )
        .route("/api/mem/graph/path", get(mem_routes::mem_graph_path))
        .route("/api/mem/graph/tunnels", get(mem_routes::mem_graph_tunnels))
        .route("/api/mem/graph/alias", post(mem_routes::mem_graph_alias))
        .route(
            "/api/mem/graph/repair/find-relations",
            post(mem_routes::mem_graph_repair_find_relations),
        )
        .route(
            "/api/mem/graph/repair/delete-relations",
            post(mem_routes::mem_graph_repair_delete_relations),
        )
        .route(
            "/api/mem/graph/repair/merge-relations",
            post(mem_routes::mem_graph_repair_merge_relations),
        )
        .route(
            "/api/mem/graph/repair/find-entities",
            post(mem_routes::mem_graph_repair_find_entities),
        )
        .route(
            "/api/mem/graph/repair/augment-entity",
            post(mem_routes::mem_graph_repair_augment_entity),
        )
        .route(
            "/api/mem/graph/repair/delete-entity",
            post(mem_routes::mem_graph_repair_delete_entity),
        )
        .route(
            "/api/mem/graph/repair/orphan-vectors",
            post(mem_routes::mem_graph_repair_orphan_vectors),
        )
        .route(
            "/api/mem/graph/repair/prune-thin-vectors",
            post(mem_routes::mem_graph_prune_thin_vectors),
        )
        .route(
            "/api/mem/graph/repair/collapse-synonym-edges",
            post(mem_routes::mem_graph_collapse_synonym_edges),
        )
        .route(
            "/api/mem/graph/repair/prune-dream-hub-edges",
            post(mem_routes::mem_graph_prune_dream_hub_edges),
        )
        .route(
            "/api/mem/graph/repair/orphan-blobs",
            post(mem_routes::mem_graph_repair_orphan_blobs),
        )
        .route("/api/mem/learn", post(mem_routes::mem_learn))
        .route("/api/mem/promote/{id}", post(mem_routes::mem_promote))
        .route("/api/mem/flush", post(mem_routes::mem_flush))
        .route(
            "/api/mem/check-duplicate",
            post(mem_routes::mem_check_duplicate),
        )
        .route("/api/mem/dream", post(mem_routes::mem_dream))
        .route(
            "/api/mem/extract-pending",
            post(mem_routes::mem_extract_pending),
        )
        .route("/api/mem/dream/report", get(mem_routes::mem_dream_report))
        .route("/api/mem/graph/full", get(mem_routes::mem_graph_full))
        .route("/api/mem/graph/labels", get(mem_routes::mem_graph_labels))
        .route("/api/mem/graph/edges", get(mem_routes::mem_graph_edges))
        .route(
            "/api/mem/graph/layout/rebuild",
            post(mem_routes::mem_graph_layout_rebuild),
        )
        .route(
            "/api/mem/pinned-positions",
            get(mem_routes::mem_pinned_positions_list),
        )
        .route(
            "/api/mem/pinned-positions/{mem_id}",
            axum::routing::patch(mem_routes::mem_pinned_positions_set)
                .delete(mem_routes::mem_pinned_positions_delete),
        )
        .route(
            "/api/mem/activity/recent",
            get(mem_routes::mem_activity_recent),
        )
        .route(
            "/api/mem/calibration/sweep",
            post(mem_routes::mem_calibration_sweep),
        )
        .route(
            "/api/mem/calibration/log",
            get(mem_routes::mem_calibration_log),
        )
        .route(
            "/api/mem/workers",
            get(mem_routes::workers_list).post(mem_routes::workers_spawn),
        )
        .route(
            "/api/mem/workers/{id}",
            get(mem_routes::workers_get).delete(mem_routes::workers_kill),
        )
        .route(
            "/api/mem/workers/{id}/progress",
            post(mem_routes::workers_progress),
        )
        .route(
            "/api/mem/workers/{id}/report",
            post(mem_routes::workers_report),
        )
        .route("/api/mem/workers/{id}/log", get(mem_routes::workers_log))
        .route("/api/mem/sync", post(mem_routes::mem_sync))
        .route("/api/mem/sync/status", get(mem_routes::mem_sync_status))
        .route("/api/plugins/snapshot", get(web_routes::plugin_snapshot))
        .route("/api/plugins/action", post(web_routes::plugin_action))
        .route("/api/plugins/glance", post(web_routes::plugin_glance))
        .route("/api/plugins/bubble", post(web_routes::plugin_bubble))
        .route("/api/plugins/view-call", post(web_routes::plugin_view_call))
        .route(
            "/api/plugins/chatbackend/session-info",
            post(web_routes::chatbackend_session_info),
        )
        .route(
            "/api/plugins/chatbackend/switch-model",
            post(web_routes::chatbackend_switch_model),
        )
        .route("/api/plugins/{id}/ui/{*path}", get(web_routes::plugin_ui))
        .route("/api/secrets/status", get(crate::secrets::api::status))
        .route("/api/secrets/unlock", post(crate::secrets::api::unlock))
        .route("/api/secrets/lock", post(crate::secrets::api::lock))
        .route("/api/secrets/logout", post(crate::secrets::api::logout))
        .route(
            "/api/secrets/items",
            get(crate::secrets::api::list_items).post(crate::secrets::api::save_item),
        )
        .route(
            "/api/secrets/items/{name}",
            get(crate::secrets::api::get_item_metadata).delete(crate::secrets::api::delete_item),
        )
        .route(
            "/api/secrets/items/{name}/value",
            post(crate::secrets::api::reveal_value),
        )
        .route(
            "/api/secrets/organizations",
            get(crate::secrets::api::organizations),
        )
        .route(
            "/api/secrets/collections",
            get(crate::secrets::api::collections),
        )
        .route(
            "/api/secrets/backend",
            post(crate::secrets::api::switch_backend),
        )
        .route("/api/secrets/sync", post(crate::secrets::api::sync))
        .route(
            "/api/secrets/auto-unlock",
            post(crate::secrets::api::auto_unlock),
        )
        .route("/api/usage/latest", get(usage_routes::usage_latest))
        .route("/api/usage/history", get(usage_routes::usage_history))
        .route(
            "/api/usage/rollup/daily",
            get(usage_routes::usage_daily_rollup),
        )
        .route("/api/usage/projection", get(usage_routes::usage_projection))
        .route("/api/usage/refresh", post(usage_routes::usage_refresh))
        .route(
            "/api/logs",
            get(logs_routes::list_logs).delete(logs_routes::delete_logs),
        );

    // gzip-scoped snapshot route: 100k-node payloads compress ~4:1. Kept on its
    // own sub-router so compression only runs for this large response.
    let router = router.merge(
        Router::new()
            .route(
                "/api/mem/graph/snapshot",
                get(mem_routes::mem_graph_snapshot),
            )
            .layer(CompressionLayer::new()),
    );

    // Web mirror: serve the built React bundle at /web with an SPA fallback (deep links resolve to index.html). Public route — the auth middleware
    // skips /web; all API calls remain token-gated.
    let router = if let Some(dist) = &state.web_dist {
        let serve = tower_http::services::ServeDir::new(dist).fallback(
            tower_http::services::ServeFile::new(dist.join("index.html")),
        );
        // no-cache = revalidate every load (304s via Last-Modified). Installed
        // iOS web apps otherwise heuristically cache index.html and miss updates.
        let serve = tower_http::set_header::SetResponseHeader::overriding(
            serve,
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        );
        router
            .route("/web/manifest.webmanifest", get(web_routes::web_manifest))
            .route(
                "/web/manifest-bootstrap",
                axum::routing::post(web_routes::web_manifest_bootstrap),
            )
            .nest_service("/web", serve)
    } else {
        router
    };

    router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            super::auth::auth_middleware,
        ))
        .layer(CorsLayer::permissive())
        .layer(CatchPanicLayer::new())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;
    use tower_http::catch_panic::CatchPanicLayer;

    #[tokio::test]
    async fn catch_panic_layer_keeps_router_responsive_after_handler_panic() {
        let app = Router::new()
            .route(
                "/panic",
                get(|| async {
                    panic!("synthetic handler panic");
                    #[allow(unreachable_code)]
                    "never"
                }),
            )
            .route("/health", get(|| async { "ok" }))
            .layer(CatchPanicLayer::new());

        let panic_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/panic")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("panic should be converted to HTTP 500 response");
        assert_eq!(
            panic_response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );

        let health_response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("router must keep serving requests after a handler panic");
        assert_eq!(health_response.status(), axum::http::StatusCode::OK);
    }
}

/// Resolve a pane ID path parameter: supports both direct IDs and aliases.
pub(crate) fn resolve_pane(
    state: &AppState,
    raw_id: &str,
) -> Result<String, (StatusCode, &'static str)> {
    let mux = state.svc.mux.write();
    mux.resolve_pane_id(raw_id)
        .ok_or((StatusCode::NOT_FOUND, "Pane not found"))
}

/// Auth + access check handled by auth_middleware layer.
/// Kept as no-op for backwards compatibility with existing handlers.
pub(crate) fn check_auth(
    _state: &AppState,
    _headers: &HeaderMap,
) -> Result<(), (StatusCode, &'static str)> {
    Ok(())
}

// Re-export route handlers so modules like commands/update/secrets keep stable imports.
pub use super::network_routes::local_ip_address;
