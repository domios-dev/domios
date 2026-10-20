pub use codeterm_sentinel::{
    agent_stage, amend, classification_policy, cleanup, command, delete, escalation, handlers,
    input, machine, portable, probe, progress, provenance, public_definition, session,
    stage_source, storage, store, terminal,
};
pub(crate) mod actions;
pub mod blueprint_pipeline;
pub mod blueprint_portable;
mod ports_impl;
pub mod service_adapter;
pub use blueprint_pipeline::BlueprintExt;
pub use blueprint_portable::BlueprintPortableExt;
#[cfg(any(test, feature = "test-support"))]
pub use ports_impl::manage_fixture_ports;
pub use service_adapter::delete::SentinelDeleteExt;
pub use service_adapter::graph::{AutomationFilesExt, AutomationGraphExt};
pub use service_adapter::SentinelExt;
pub(crate) mod classify;
pub mod event_subscription;
pub mod graph;
mod pipeline;
mod probe_env;
pub(crate) mod routes;
pub(crate) mod run_binding;
pub mod scheduler;
#[cfg(test)]
mod session_tests;
pub(crate) mod source;
#[cfg(test)]
mod storage_tests;
#[cfg(test)]
mod terminal_tests;
#[cfg(test)]
mod tests;
pub(crate) mod wake;

#[cfg(test)]
mod connected_tests;

#[cfg(test)]
mod provenance_tests;

#[cfg(test)]
mod scheduler_tests;

#[cfg(test)]
mod graph_tests;

#[cfg(test)]
mod command_tests;

#[cfg(test)]
mod escalation_tests;
#[cfg(test)]
mod persistence_roundtrip_tests;
#[cfg(test)]
#[path = "run_binding_tests.rs"]
mod run_binding_tests;

extern crate domios_analytics as codeterm_analytics; // rename-bridge
extern crate domios_blueprint_types as codeterm_blueprint_types; // rename-bridge
extern crate domios_cron as codeterm_cron; // rename-bridge
extern crate domios_decision as codeterm_decision; // rename-bridge
extern crate domios_events as codeterm_events; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_pty as codeterm_pty; // rename-bridge
extern crate domios_sentinel as codeterm_sentinel; // rename-bridge
extern crate domios_service_core as codeterm_service_core; // rename-bridge
extern crate domios_shutdown as codeterm_shutdown; // rename-bridge
extern crate domios_storage as codeterm_storage; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
