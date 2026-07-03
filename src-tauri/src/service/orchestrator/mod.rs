//! Agent spawn & batch command orchestration.

mod batch;
mod launch;
mod spawn;
mod types;

pub use launch::{
    build_agent_command, initial_task_delivery_confirmed, launch_agent_in_background,
    role_onboarding_task, skip_local_worker_role, AgentLaunchParams, AgentRegistry,
    ReadinessStrategy, TaskDelivery,
};
pub use types::{BatchOp, SpawnAgentParams, SpawnResult};

#[cfg(test)]
mod tests;
