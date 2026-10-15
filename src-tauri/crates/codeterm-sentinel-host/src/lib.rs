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
pub use ports_impl::register_sentinel;
pub use service_adapter::delete::SentinelDeleteExt;
pub use service_adapter::graph::{AutomationFilesExt, AutomationGraphExt};
pub use service_adapter::SentinelExt;
pub(crate) mod classify;
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
