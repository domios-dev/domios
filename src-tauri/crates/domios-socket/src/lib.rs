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
            dispatch_service_rpc: Box::new(server::dispatch_service_rpc),
            dispatch_mesh_rpc: Box::new(server::dispatch_mesh_rpc),
            recover_remote_attempts: remote_recovery::recover_remote_attempts,
            respawn_remote: remote_recovery::respawn_remote,
            run_dispatch_startup: |startup, ready_rx| {
                server::plan_task_dispatch::startup::run_startup(startup, ready_rx)
            },
            start_server: Box::new(server::SocketServer::start),
        },
    );
}

#[cfg(test)]
extern crate domios_api as codeterm_api; // rename-bridge
extern crate domios_blueprint_types as codeterm_blueprint_types; // rename-bridge
extern crate domios_bridge as codeterm_bridge; // rename-bridge
extern crate domios_build_info as codeterm_build_info; // rename-bridge
extern crate domios_events as codeterm_events; // rename-bridge
extern crate domios_mem as codeterm_mem; // rename-bridge
extern crate domios_notification_core as codeterm_notification_core; // rename-bridge
extern crate domios_orchestration as codeterm_orchestration; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plan as codeterm_plan; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_sentinel as codeterm_sentinel; // rename-bridge
extern crate domios_sentinel_host as codeterm_sentinel_host; // rename-bridge
extern crate domios_service_core as codeterm_service_core; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
