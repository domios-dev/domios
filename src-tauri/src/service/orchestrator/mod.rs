//! Agent spawn & batch command orchestration.

mod batch;
mod launch;
pub mod refresh;
mod spawn;
mod types;

pub(crate) use launch::{build_agent_command, launch_agent_in_background, role_onboarding_task};
#[cfg(test)]
pub(crate) use launch::{skip_local_worker_role, AgentRegistry};
pub use refresh::{RefreshOutcome, RefreshRequest, RefreshTrigger, RefreshTuning};
pub use types::{BatchOp, SpawnAgentParams, SpawnResult};

#[cfg(test)]
mod tests;
