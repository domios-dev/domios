//! Memory-worker spawn routing.
//!
//! Verdict (b) for this path: a chat backend genuinely cannot serve here. The
//! worker contract is closed by `codeterm mem worker report`, whose only handle
//! is `$CODETERM_WORKER_ID` (`cli/src/cmd/mem/worker.rs`), and that variable is
//! exported solely by the PTY launch env (`service/spawn.rs`
//! `spawn_hidden_agent_impl`). `TabCommand::ChatBackend` has no worker-identity
//! slot at all and the chat exec env carries none, so a chat-backend worker
//! would register, start, and never be able to report — reaped as `Orphaned`.
//! The routing decision itself is still delegated to the registry seam; only the
//! stamp of the worker identity can fail, and it fails naming the capability.

use crate::chatbackend::ChatBackendRegistry;
use crate::multiplexer::types::TabAudience;
use crate::service::spawn::{SpawnTabSpec, TabVisibility};
use crate::service::spawn_capability::{MissingSpawnCapability, SpawnRejection};
use crate::service::SpawnAgentParams;

#[cfg(test)]
#[path = "worker_spawn_spec/tests.rs"]
mod tests;

pub(crate) struct MemWorkerSpawnInputs {
    pub(crate) agent_provider: String,
    pub(crate) model: String,
    pub(crate) task: String,
    pub(crate) worker_id: String,
    pub(crate) spawned_by: String,
    pub(crate) intent: String,
}

pub(crate) fn build_mem_worker_spawn_spec(
    chatbackends: &ChatBackendRegistry,
    inputs: MemWorkerSpawnInputs,
) -> Result<SpawnTabSpec, SpawnRejection> {
    let params = SpawnAgentParams {
        agent: Some(inputs.agent_provider.clone()),
        model: (!inputs.model.is_empty()).then_some(inputs.model),
        task: Some(inputs.task),
        title: Some(format!("mem:{}", inputs.intent)),
        audience: TabAudience::Automation,
        ..Default::default()
    };

    let mut spec = SpawnTabSpec::from_agent_params_with_chatbackends(chatbackends, params);
    spec.visibility = TabVisibility::Hidden;
    spec.bind_worker_identity(&inputs.agent_provider, &inputs.worker_id)
        .inspect_err(|rejection| {
            log::warn!(
                "[worker] '{}' cannot serve a memory worker: missing capability {}",
                rejection.requested(),
                rejection
                    .missing_capability()
                    .map(MissingSpawnCapability::id)
                    .unwrap_or("unregistered")
            );
        })?;
    if let Some(agent) = spec.agent_launch_mut() {
        agent.spawned_by = Some(inputs.spawned_by);
    }
    Ok(spec)
}
