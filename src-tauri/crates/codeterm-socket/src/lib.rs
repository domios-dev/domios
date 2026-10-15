pub mod server;

use server::plan_task_dispatch::remote_recovery;
use server::protocol;

pub fn register_socket(app: codeterm_host_runtime::app_runtime::AppHandle) {
    let service_app = app.clone();
    let mesh_app = app.clone();
    codeterm_service_core::ports::socket::register(
        codeterm_service_core::ports::socket::RpcBridge {
            protocol_version: || protocol::PROTOCOL_VERSION,
            version_skew_gate: |raw| {
                protocol::version_skew_gate(
                    protocol::client_protocol_version(raw),
                    protocol::PROTOCOL_VERSION,
                    protocol::RECOVERY_COMMAND,
                )
            },
            dispatch_service_rpc: Box::new(move |method, params, svc| {
                server::dispatch_service_rpc(method, params, svc, Some(&service_app))
            }),
            dispatch_mesh_rpc: Box::new(move |method, params, svc, peer| {
                server::dispatch_mesh_rpc(method, params, svc, peer, Some(&mesh_app))
            }),
            recover_remote_attempts: remote_recovery::recover_remote_attempts,
            respawn_remote: remote_recovery::respawn_remote,
            run_dispatch_startup: |startup, ready_rx| {
                server::plan_task_dispatch::startup::run_startup(startup, ready_rx)
            },
            start_server: Box::new(move |mux, pty, notif, output, svc| {
                server::SocketServer::start(mux, pty, notif, app.clone(), output, svc)
            }),
        },
    );
}
