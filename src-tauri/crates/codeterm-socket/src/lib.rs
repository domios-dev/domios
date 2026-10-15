pub mod server;

use server::plan_task_dispatch::remote_recovery;
use server::protocol;

pub fn register_socket() {
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
            dispatch_service_rpc: server::dispatch_service_rpc,
            dispatch_mesh_rpc: server::dispatch_mesh_rpc,
            recover_remote_attempts: remote_recovery::recover_remote_attempts,
            respawn_remote: remote_recovery::respawn_remote,
            run_dispatch_startup: |startup, ready_rx| {
                server::plan_task_dispatch::startup::run_startup(startup, ready_rx)
            },
            start_server: server::SocketServer::start,
        },
    );
}
