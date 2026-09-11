use axum::{
    http::{HeaderMap, StatusCode},
    middleware,
    routing::{delete, get, patch, post},
    Router,
};
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    cors::{AllowOrigin, Any, CorsLayer},
};

use super::{
    agent_done_routes, agent_routes, bridge_routes, clipboard_routes, config_routes, cron_routes,
    debug_routes, fs_routes, health_routes, inbound_webhook, logs_routes, mem_routes,
    mesh_inventory, mesh_routes, network_routes, notification_routes, output_stream,
    pasteboard_routes, plan_routes, pty_routes, services_routes, session_routes, tab_agent_routes,
    tab_routes, task_routes, team_routes, theme_routes, updater_routes, usage_routes, web_routes,
    webhook_routes, workspace_routes,
};

/// Origins Tauri serves the app window from: custom scheme on macOS/iOS/Linux,
/// `tauri.localhost` on Windows/Android.
const APP_WEBVIEW_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// The app's own window is a first-party client of this daemon yet cross-origin to
/// it, so without an explicit allow every raw fetch from the UI — the remote-tab
/// SSE streams — is rejected for a missing `Access-Control-Allow-Origin`.
fn with_app_webview_origins(
    mut origins: Vec<axum::http::HeaderValue>,
) -> Vec<axum::http::HeaderValue> {
    origins.extend(
        APP_WEBVIEW_ORIGINS
            .iter()
            .filter_map(|origin| origin.parse::<axum::http::HeaderValue>().ok()),
    );
    origins
}

fn cors_layer(cors_enabled: bool, configured_origins: &[String]) -> CorsLayer {
    let mut origins = if cors_enabled {
        configured_origins
            .iter()
            .filter_map(|origin| origin.parse::<axum::http::HeaderValue>().ok())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    #[cfg(debug_assertions)]
    if !cors_enabled {
        origins.extend(debug_vite_origins());
    }
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(with_app_webview_origins(origins)))
        .allow_methods(Any)
        .allow_headers(Any)
}

#[cfg(debug_assertions)]
fn debug_vite_origins() -> Vec<axum::http::HeaderValue> {
    let Some(port) = std::env::var("CODETERM_VITE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
    else {
        return Vec::new();
    };
    [
        format!("http://localhost:{port}"),
        format!("http://127.0.0.1:{port}"),
    ]
    .into_iter()
    .filter_map(|origin| origin.parse::<axum::http::HeaderValue>().ok())
    .collect()
}

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
    /// Lets the config apply path compare a persisted bind against the live socket.
    pub(crate) rest_api: crate::startup::RestApiControlState,
    /// Some only in debug builds started with CODETERM_TEST_CLOCK=1 —
    /// powers /api/debug/clock time travel for the cron e2e suite.
    pub test_clock: Option<std::sync::Arc<crate::cron::clock::MockClock>>,
    /// System event bus — feeds the web mirror's SSE stream.
    pub events: crate::api::events::EventBroadcasterState,
    /// Directory holding the built React bundle, served at the daemon root.
    pub web_dist: Option<std::path::PathBuf>,
    /// Caps concurrent full-state snapshots so a request storm cannot take the
    /// daemon down. See `api::snapshot_bound`.
    pub snapshot_bound: crate::api::snapshot_bound::SnapshotBound,
}

#[derive(Clone)]
pub struct LocalApiRouter(pub Router);

/// Lifetime of the `codeterm-web-token` PWA bootstrap cookie (web_routes).
pub(crate) const WEB_TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

async fn legacy_web_route_gone() -> StatusCode {
    StatusCode::NOT_FOUND
}

pub fn create_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/api/health", get(health_routes::health))
        .route("/api/identify", get(health_routes::identify))
        .route("/api/fs/list-dir", get(fs_routes::list_dir))
        .route("/api/fs/mkdir", post(fs_routes::mkdir))
        .route(
            "/api/teams/{team_id}/rename",
            post(team_routes::rename_team),
        )
        .route("/api/teams/{team_id}/close", post(team_routes::close_team))
        .route(
            "/api/teams/{team_id}/members",
            post(team_routes::add_team_member),
        )
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
            post(workspace_routes::reopen_closed_tab),
        )
        .route(
            "/api/closed-groups/{group_id}/reopen",
            post(workspace_routes::reopen_closed_group),
        )
        .route(
            "/api/workspaces/reorder",
            post(workspace_routes::reorder_workspaces),
        )
        .route(
            "/api/workspaces/{id}/tabs",
            post(workspace_routes::create_tab),
        )
        .route(
            "/api/workspaces/{id}/tabs/reorder",
            post(workspace_routes::reorder_tabs),
        )
        .route(
            "/api/tabs/{id}/children/reorder",
            post(workspace_routes::reorder_children),
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
            "/api/tabs",
            get(tab_routes::list_tabs).post(tab_routes::create_shell_tab),
        )
        .route("/api/tabs/inspect", get(tab_routes::inspect_tabs))
        .route(
            "/api/plans",
            get(plan_routes::list).post(plan_routes::create),
        )
        .route("/api/plans/resolve-refs", post(plan_routes::resolve_refs))
        .route(
            "/api/specs",
            get(plan_routes::spec::list).post(plan_routes::spec::create),
        )
        .route("/api/specs/{id}", get(plan_routes::spec::get))
        .route(
            "/api/specs/{id}/sections",
            post(plan_routes::spec::section_add),
        )
        .route(
            "/api/specs/{id}/sections/{sid}",
            patch(plan_routes::spec::section_update).delete(plan_routes::spec::section_rm),
        )
        .route("/api/specs/{id}/links", get(plan_routes::spec::link_list))
        .route("/api/efforts/{id}", get(plan_routes::spec::effort_show))
        .route(
            "/api/efforts/{id}/links",
            post(plan_routes::spec::effort_link),
        )
        .route(
            "/api/plans/{id}",
            get(plan_routes::get)
                .patch(plan_routes::update)
                .delete(plan_routes::delete_plan),
        )
        .route("/api/plans/{id}/archive", post(plan_routes::archive))
        .route("/api/plans/{id}/unarchive", post(plan_routes::unarchive))
        .route("/api/plans/{id}/pause", post(plan_routes::pause))
        .route("/api/plans/{id}/resume", post(plan_routes::resume))
        .route("/api/plans/{id}/mode", post(plan_routes::mode_set))
        .route("/api/plans/{id}/reconcile", get(plan_routes::reconcile))
        .route("/api/plans/{id}/state", get(plan_routes::state))
        .route("/api/plans/{id}/next", get(plan_routes::next))
        .route("/api/plans/{id}/attachments", get(plan_routes::attachment))
        .route("/api/plans/{id}/transfer", post(plan_routes::transfer))
        .route("/api/plans/{id}/disown", post(plan_routes::disown))
        .route("/api/plans/{id}/claim", post(plan_routes::claim))
        .route("/api/plans/{id}/ingest", post(plan_routes::ingest))
        .route("/api/plans/{id}/skills", get(plan_routes::skills_list))
        .route(
            "/api/plans/{id}/skills/{slug}",
            get(plan_routes::skills_show),
        )
        .route(
            "/api/plans/{id}/retrospective",
            post(plan_routes::retrospective_write),
        )
        .route("/api/plans/{id}/sections", post(plan_routes::section_add))
        .route(
            "/api/plans/{id}/sections/{sid}",
            patch(plan_routes::section_update).delete(plan_routes::section_rm),
        )
        .route("/api/plans/{id}/kinds", post(plan_routes::kinds_set))
        .route(
            "/api/plans/{id}/tag-facets",
            post(plan_routes::tag_facets_set),
        )
        .route("/api/plans/{id}/coverage", get(plan_routes::coverage))
        .route("/api/plans/{id}/groups", post(plan_routes::groups_set))
        .route("/api/plans/{id}/batch", post(plan_routes::batch::batch))
        .route(
            "/api/plans/{id}/handoffs",
            get(plan_routes::handoff::list).post(plan_routes::handoff::create),
        )
        .route(
            "/api/plans/{id}/handoffs/show",
            get(plan_routes::handoff::show),
        )
        .route("/api/plans/{id}/links", post(plan_routes::link))
        .route("/api/plans/{id}/links/{to}", delete(plan_routes::unlink))
        .route(
            "/api/plans/{id}/spec-links",
            post(plan_routes::spec::link_add),
        )
        .route(
            "/api/plans/{id}/spec-links/{spec_id}",
            delete(plan_routes::spec::link_rm),
        )
        .route("/api/plans/{id}/tasks", post(plan_routes::task_add))
        .route("/api/tasks", get(task_routes::list).post(task_routes::add))
        .route("/api/tasks/{id}", get(task_routes::show))
        .route("/api/tasks/{id}/take", post(task_routes::take))
        .route("/api/tasks/{id}/drop", post(task_routes::drop_task))
        .route("/api/canvas/focus", post(task_routes::canvas_focus))
        .route(
            "/api/plans/{id}/tasks/{tid}",
            patch(plan_routes::task_update).delete(plan_routes::task_rm),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/close",
            post(plan_routes::task_close),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/assign",
            post(plan_routes::task_assign),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/attempts",
            get(plan_routes::task_attempts),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/attempts/{attempt_id}",
            get(plan_routes::task_attempt_show),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/respawn",
            post(plan_routes::task_respawn),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/adopt-checkpoint",
            post(plan_routes::task_adopt_checkpoint),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/milestones",
            post(plan_routes::milestone_add),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/milestones/{ms_id}",
            patch(plan_routes::milestone_update).delete(plan_routes::milestone_rm),
        )
        .route(
            "/api/plans/{id}/decision",
            post(plan_routes::gate_ask).get(plan_routes::gate_list),
        )
        .route(
            "/api/plans/{id}/decision/{gid}/answer",
            post(plan_routes::gate_answer),
        )
        .route(
            "/api/plans/{id}/decision/{gid}/escalate",
            post(plan_routes::gate_escalate),
        )
        .route("/api/plans/{id}/loops", post(plan_routes::loop_create))
        .route(
            "/api/plans/{id}/loops/{lid}",
            get(plan_routes::loop_status).patch(plan_routes::loop_set),
        )
        .route(
            "/api/plans/{id}/loops/{lid}/stop",
            post(plan_routes::loop_stop),
        )
        .route(
            "/api/plans/{id}/loops/{lid}/record",
            post(plan_routes::loop_record),
        )
        .route("/api/plans/{id}/layout", patch(plan_routes::layout_set))
        .route("/api/plans/{id}/notes", post(plan_routes::note_add))
        .route(
            "/api/plans/{id}/notes/{element_id}",
            patch(plan_routes::note_update).delete(plan_routes::element_rm),
        )
        .route("/api/plans/{id}/elements", post(plan_routes::element_add))
        .route(
            "/api/plans/{id}/elements/{element_id}",
            patch(plan_routes::element_update).delete(plan_routes::element_rm),
        )
        .route("/api/app/restart", post(network_routes::restart_app))
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
            "/api/notifications/channels",
            get(notification_routes::push_channel_roster),
        )
        .route(
            "/api/notifications/{id}/read",
            post(notification_routes::mark_notification_read),
        )
        .route(
            "/api/notifications/{id}/answer",
            post(notification_routes::answer_notification),
        )
        .route(
            "/api/notifications/{id}",
            axum::routing::delete(notification_routes::delete_notification),
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
        .route("/api/debug/clock", get(debug_routes::clock_status))
        .route(
            "/api/debug/clock/advance",
            post(debug_routes::clock_advance),
        )
        .route("/api/debug/events/stats", get(debug_routes::event_stats))
        .route(
            "/api/debug/events/force-latch",
            post(debug_routes::force_latch),
        )
        .route(
            "/api/debug/events/reset-latch",
            post(debug_routes::reset_latch),
        );
    let router = router.route(
        "/api/debug/shutdown/clean-restart",
        post(debug_routes::clean_restart),
    );
    let router = tab_agent_routes::mount(router);
    #[cfg(debug_assertions)]
    let router = router.route(
        "/api/dev/inject-cast",
        post(super::dev_cast_routes::inject_cast),
    );
    let router = router
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
        .route("/api/tabs/{id}/chat-view", post(web_routes::tab_chat_view))
        .route(
            "/api/mesh/peers/{id}/services",
            get(web_routes::peer_services),
        )
        .route("/api/mesh/inventory", get(mesh_inventory::inventory))
        .route(
            "/api/mesh/peers/{id}/services/{service_id}",
            delete(web_routes::peer_service_delete),
        )
        .route(
            "/api/mesh/peers/{id}/crons/{cron_id}",
            get(web_routes::peer_cron_get)
                .put(web_routes::peer_cron_update)
                .delete(web_routes::peer_cron_delete),
        )
        .route(
            "/api/mesh/peers/{id}/crons/{cron_id}/{action}",
            post(web_routes::peer_cron_action),
        )
        .route(
            "/api/mesh/peers/{id}/crons/{cron_id}/history",
            get(web_routes::peer_cron_history),
        )
        .route("/api/events/stream", get(web_routes::events_stream))
        .route("/api/output/stream", get(output_stream::output_stream))
        .route("/api/transcribe-voice", post(web_routes::transcribe_voice))
        .route(
            "/api/tabs/{id}/chat-raw/start",
            post(web_routes::chat_raw_start),
        )
        .route(
            "/api/tabs/{id}/chat-raw/stop",
            post(web_routes::chat_raw_stop),
        )
        .route(
            "/api/tabs/{id}/chat-raw/backfill",
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
        .route("/api/pty/spawn", post(pty_routes::pty_spawn))
        .route("/api/pty/{id}/read", get(pty_routes::pty_read))
        .route("/api/pty/{id}/write", post(pty_routes::pty_write))
        .route("/api/pty/{id}/resize", post(pty_routes::pty_resize))
        .route(
            "/api/pty/{id}/launch-command",
            post(pty_routes::pty_launch_command),
        )
        .route("/api/session/save", post(session_routes::save_session_api))
        .route(
            "/api/session/backups",
            get(session_routes::list_session_backups_api),
        )
        .route(
            "/api/session/backups/{name}/restore",
            post(session_routes::restore_session_backup_api),
        )
        .route("/api/providers", get(agent_routes::list_agent_providers))
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
        .route("/api/agents/done", get(agent_done_routes::list_done))
        .route(
            "/api/orchestration/new",
            post(agent_routes::create_orchestration_pair),
        )
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
                .patch(services_routes::update_service)
                .delete(services_routes::delete_service),
        )
        .route(
            "/api/services/{id}/regenerate-token",
            post(services_routes::regenerate_token),
        )
        .route(
            "/api/services/smart-add-inspect",
            post(services_routes::smart_add_inspect),
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
        .route("/api/mesh/relay-to", post(mesh_routes::mesh_relay_to_peer))
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
        .route("/api/mesh/enabled", post(mesh_routes::mesh_set_enabled))
        .route(
            "/api/mesh/broadcast/start",
            post(mesh_routes::mesh_start_broadcast),
        )
        .route(
            "/api/mesh/broadcast/stop",
            post(mesh_routes::mesh_stop_broadcast),
        )
        .route("/api/mesh/relay", post(mesh_routes::mesh_relay))
        .route("/api/mesh/rpc", post(mesh_routes::mesh_rpc))
        .route(
            "/api/mesh/relay-stream",
            get(mesh_routes::mesh_relay_stream),
        )
        .route(
            "/api/mesh/relay-asset/{peer}/{*path}",
            get(mesh_routes::mesh_relay_asset),
        )
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
        .route("/api/mesh/exit", post(mesh_routes::mesh_exit))
        .route(
            "/api/mesh/peers/{id}/access",
            post(mesh_routes::mesh_set_access),
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
        .route("/api/mem/items/{id}/append", post(mem_routes::mem_append))
        .route("/api/mem/items/{id}/restore", post(mem_routes::mem_restore))
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
            "/api/repo-watch/subscribe",
            post(web_routes::repo_watch_subscribe),
        )
        .route(
            "/api/repo-watch/unsubscribe",
            post(web_routes::repo_watch_unsubscribe),
        )
        .route(
            "/api/plugins/chatbackend/session-info",
            post(web_routes::chatbackend_session_info),
        )
        .route(
            "/api/plugins/chatbackend/switch-model",
            post(web_routes::chatbackend_switch_model),
        )
        .route(
            "/api/plugins/chatbackend/open",
            post(web_routes::open_chat_backend_tab),
        )
        .route("/api/plugins/{id}/ui/{*path}", get(web_routes::plugin_ui))
        .route(
            "/api/plugins/{plugin_id}/webhook",
            post(inbound_webhook::receive_webhook).layer(axum::extract::DefaultBodyLimit::max(
                inbound_webhook::MAX_WEBHOOK_BODY_BYTES,
            )),
        )
        .route(
            "/api/attachments/temp-file",
            post(web_routes::save_temp_file),
        )
        .route(
            "/api/attachments/clipboard-image",
            post(clipboard_routes::save_clipboard_image),
        )
        .route(
            "/api/attachments/clipboard-file",
            post(clipboard_routes::save_clipboard_file),
        )
        .route(
            "/api/clipboard/copy-image",
            post(web_routes::copy_image_to_clipboard),
        )
        .route(
            "/api/agents/general/restart",
            post(agent_routes::restart_general_agent),
        )
        .route(
            "/api/agents/general/{tab_id}/restart",
            post(agent_routes::restart_general_agent_tab),
        )
        .route(
            "/api/agents/general/complete-first-run",
            post(agent_routes::complete_general_agent_first_run),
        )
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

    // own sub-router so compression only runs for this large response.
    let router = router.merge(
        Router::new()
            .route(
                "/api/mem/graph/snapshot",
                get(mem_routes::mem_graph_snapshot),
            )
            .layer(CompressionLayer::new()),
    );

    let router = if let Some(dist) = &state.web_dist {
        let root_serve = tower_http::services::ServeDir::new(dist).fallback(
            tower_http::services::ServeFile::new(dist.join("index.html")),
        );
        // iOS web apps otherwise heuristically cache index.html and miss updates.
        let root_serve = tower_http::set_header::SetResponseHeader::overriding(
            root_serve,
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        );
        router
            .route("/manifest.webmanifest", get(web_routes::web_manifest))
            .route(
                "/manifest-bootstrap",
                axum::routing::post(web_routes::web_manifest_bootstrap),
            )
            .route("/web", get(legacy_web_route_gone))
            .route("/web/{*path}", get(legacy_web_route_gone))
            .fallback_service(root_serve)
    } else {
        router
    };

    let api_config = state.config.lock().api.clone();
    let router = router.layer(middleware::from_fn_with_state(
        state.clone(),
        super::auth::auth_middleware,
    ));
    let router = router.layer(cors_layer(api_config.cors, &api_config.cors_allow_origins));
    router.layer(CatchPanicLayer::new()).with_state(state)
}

#[cfg(test)]
mod tests;

/// Resolve a tab ID path parameter: supports both direct IDs and aliases.
pub(crate) fn resolve_tab(
    state: &AppState,
    raw_id: &str,
) -> Result<String, (StatusCode, &'static str)> {
    let mux = state.svc.mux.write();
    mux.resolve_tab_id(raw_id)
        .ok_or((StatusCode::NOT_FOUND, "Tab not found"))
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
