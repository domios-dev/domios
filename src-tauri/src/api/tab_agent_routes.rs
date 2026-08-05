use axum::{
    routing::{get, post},
    Router,
};

use super::{
    agent_routes, delayed_send_routes, hook_routes, pty_routes, routes::AppState, tab_chat_routes,
    tab_routes,
};

pub(crate) fn mount(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/api/tabs/{id}/send", post(tab_routes::send_text))
        .route("/api/tabs/{id}/chat", get(tab_chat_routes::snapshot))
        .route(
            "/api/tabs/{id}/chat-send",
            post(tab_routes::chat_send_text).get(tab_routes::chat_send_status),
        )
        .route("/api/tabs/{id}/stop", post(tab_routes::stop_tab))
        .route("/api/tabs/{id}/focus", post(tab_routes::focus_tab))
        .route("/api/tabs/{id}/close", post(tab_routes::close_tab))
        .route(
            "/api/tabs/{id}/restart",
            post(tab_routes::restart_shell_tab),
        )
        .route("/api/tabs/{id}/rename", post(tab_routes::rename_tab))
        .route("/api/tabs/{id}/describe", post(tab_routes::describe_tab))
        .route("/api/tabs/{id}/output", get(tab_routes::get_tab_output))
        .route(
            "/api/tabs/{id}/output/stream",
            get(tab_routes::stream_tab_output),
        )
        .route(
            "/api/agent-runs/{tab_id}/log",
            get(tab_routes::agent_run_log),
        )
        .route("/api/tabs/{id}/text", get(tab_routes::get_tab_text))
        .route(
            "/api/tabs/{id}/shell-commands",
            get(tab_routes::shell_command_snapshot),
        )
        .route("/api/tabs/{id}/usage", get(tab_routes::get_tab_usage))
        .route("/api/tabs/{id}/exec", post(tab_routes::exec_in_tab))
        .route(
            "/api/tabs/{id}/capture/start",
            post(tab_routes::start_capture),
        )
        .route(
            "/api/tabs/{id}/capture/stop",
            post(tab_routes::stop_capture),
        )
        .route("/api/tabs/{id}/resize", post(tab_routes::resize_tab))
        .route(
            "/api/tabs/{id}/resize-owner",
            post(pty_routes::claim_resize_owner),
        )
        .route("/api/tabs/{id}/link-pty", post(tab_routes::link_pty))
        .route(
            "/api/agents/providers/{id}/install-in-tab",
            post(agent_routes::install_provider_in_tab),
        )
        .route(
            "/api/tabs/{id}/delayed-messages",
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
        .route(
            "/api/tabs/{id}/role",
            post(agent_routes::set_tab_role).delete(agent_routes::clear_tab_role),
        )
        .route(
            "/api/tabs/{id}/make-orchestrator",
            post(agent_routes::make_orchestrator),
        )
        .route(
            "/api/tabs/{id}/make-planner",
            post(agent_routes::make_planner),
        )
        .route("/api/tabs/{id}/status", get(agent_routes::get_tab_status))
        .route("/api/tabs/{id}/report", post(agent_routes::submit_report))
        .route("/api/tabs/{id}/reports", get(agent_routes::get_reports))
        .route(
            "/api/tabs/{id}/can-spawn",
            post(agent_routes::set_can_spawn),
        )
        .route(
            "/api/tabs/{id}/agent/restart",
            post(agent_routes::agent_restart_handler),
        )
        .route(
            "/api/tabs/{id}/agent-type",
            post(agent_routes::set_agent_type),
        )
        .route(
            "/api/tabs/{id}/prompt/respond",
            post(agent_routes::respond_prompt),
        )
        .route(
            "/api/tabs/{id}/prompt/clear",
            post(agent_routes::clear_prompt),
        )
        .route(
            "/api/tabs/{id}/hook/permission",
            post(hook_routes::hook_permission),
        )
        .route("/api/tabs/{id}/hook/stop", post(hook_routes::hook_stop))
        .route(
            "/api/tabs/{id}/hook/session_start",
            post(hook_routes::hook_session_start),
        )
        .route(
            "/api/agents/providers",
            get(agent_routes::list_agent_providers),
        )
        .route(
            "/api/remote-workers",
            get(agent_routes::remote_workers)
                .post(agent_routes::record_remote_worker)
                .delete(agent_routes::forget_remote_worker),
        )
}
