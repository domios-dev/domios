pub mod account_routes;
pub mod analytics_routes;
mod attachment_upload;
pub mod auth;
pub mod automation_graph_routes;
pub mod blueprint_portable_routes;
#[cfg(debug_assertions)]
mod cors_diagnostics;
pub mod events;
#[cfg(debug_assertions)]
mod request_census;
pub mod routes;
mod runtime_login;
pub mod secrets_routes;

pub mod agent_done_routes;
pub mod agent_fork_routes;
pub mod agent_model_catalog_routes;
pub mod agent_model_routes;
pub(crate) mod agent_provider_switch_routes;
pub(crate) mod agent_restart_routes;
pub mod agent_routes;
pub mod ai_helper_routes;
pub mod bridge_routes;
pub(crate) mod cli_rpc_routes;
pub mod clipboard_routes;
pub mod config_routes;
pub mod cron_routes;
pub mod debug_routes;
pub mod delayed_send_routes;
#[cfg(debug_assertions)]
pub mod dev_cast_routes;
pub(crate) mod essence_map_routes;
pub mod frontend_bundle_routes;
pub mod frontend_shell_routes;
pub mod fs_routes;
mod general_session_routes;
pub mod health_routes;
pub mod hook_routes;
pub mod inbound_webhook;
pub(crate) mod local_preview_routes;
pub mod logs_routes;
pub mod mem_routes;
mod memory_router;
pub mod mesh_attachment;
pub mod mesh_inventory;
pub mod mesh_routes;
pub(crate) mod model_provider_routes;
pub mod network_routes;
pub mod notification_routes;
#[cfg(test)]
mod optional_json_contract_tests;
pub mod output_stream;
pub mod pasteboard_routes;
pub(crate) mod peer_redaction;
pub(crate) mod permission_auto_confirm_routes;
pub mod plan_routes;
pub mod plugin_tab_menu_routes;
pub(crate) mod provider_cli_routes;
pub mod pty_routes;
pub mod services_routes;
pub(crate) mod session_binding_routes;
pub(crate) mod session_exchange;
pub mod session_routes;
pub(crate) mod stream_census;
pub(crate) mod stream_diagnostics;
pub(crate) mod surface_routes;
pub(crate) mod tab_adopt_routes;
pub mod tab_agent_routes;
pub mod tab_chat_routes;
pub(crate) mod tab_repair_routes;
pub mod tab_routes;
mod tab_todo_routes;
pub mod task_routes;
pub mod team_mission_control_routes;
pub mod team_routes;
pub mod team_spend;
pub mod theme_routes;
pub mod transport_capabilities;
pub mod transport_orchestration;
pub mod transport_tabs;
pub mod updater_routes;
pub mod usage_routes;
pub mod web_plugin_routes;
pub mod web_routes;
pub mod webhook_routes;
pub mod workspace_routes;

mod capability_probe_routes;

#[cfg(test)]
mod core_contract_tests;

#[cfg(test)]
mod rest_liveness_tests;

#[cfg(test)]
mod process_split_contract;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub mod app_quit_routes;

extern crate domios_account as codeterm_account; // rename-bridge
extern crate domios_analytics as codeterm_analytics; // rename-bridge
extern crate domios_billing as codeterm_billing; // rename-bridge
extern crate domios_blueprint_types as codeterm_blueprint_types; // rename-bridge
extern crate domios_bridge as codeterm_bridge; // rename-bridge
extern crate domios_build_info as codeterm_build_info; // rename-bridge
extern crate domios_commands as codeterm_commands; // rename-bridge
extern crate domios_decision as codeterm_decision; // rename-bridge
extern crate domios_events as codeterm_events; // rename-bridge
extern crate domios_frontend_bundle as codeterm_frontend_bundle; // rename-bridge
extern crate domios_frontend_shell as codeterm_frontend_shell; // rename-bridge
extern crate domios_mem as codeterm_mem; // rename-bridge
extern crate domios_mesh as codeterm_mesh; // rename-bridge
extern crate domios_notification_core as codeterm_notification_core; // rename-bridge
extern crate domios_orchestration as codeterm_orchestration; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plan as codeterm_plan; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_plugins as codeterm_plugins; // rename-bridge
extern crate domios_pty as codeterm_pty; // rename-bridge
extern crate domios_sentinel_host as codeterm_sentinel_host; // rename-bridge
extern crate domios_service_core as codeterm_service_core; // rename-bridge
extern crate domios_storage as codeterm_storage; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
extern crate domios_update as codeterm_update; // rename-bridge
extern crate domios_usage as codeterm_usage; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
extern crate domios_wire as codeterm_wire; // rename-bridge
