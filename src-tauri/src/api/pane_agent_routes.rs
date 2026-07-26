use axum::{
    routing::{get, post},
    Router,
};

use super::{
    agent_routes, delayed_send_routes, hook_routes, pane_routes, pty_routes, routes::AppState,
};

pub(crate) fn mount(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/api/panes/{id}/send", post(pane_routes::send_text))
        .route(
            "/api/panes/{id}/chat-send",
            post(pane_routes::chat_send_text).get(pane_routes::chat_send_status),
        )
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
            post(pty_routes::claim_resize_owner),
        )
        .route("/api/panes/{id}/link-pty", post(pane_routes::link_pty))
        .route(
            "/api/agents/providers/{id}/install-in-pane",
            post(agent_routes::install_provider_in_pane),
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
        .route(
            "/api/panes/{id}/role",
            post(agent_routes::set_pane_role).delete(agent_routes::clear_pane_role),
        )
        .route(
            "/api/panes/{id}/make-orchestrator",
            post(agent_routes::make_orchestrator),
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
        .route(
            "/api/remote-workers",
            get(agent_routes::remote_workers)
                .post(agent_routes::record_remote_worker)
                .delete(agent_routes::forget_remote_worker),
        )
}
