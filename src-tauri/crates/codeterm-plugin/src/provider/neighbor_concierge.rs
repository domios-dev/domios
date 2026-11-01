use super::{AgentProvider, SystemPromptDelivery};
use codeterm_types::plan::ExecutionIntent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeighborConciergeCapability {
    pub supported: bool,
    pub machine_access: Vec<String>,
    pub enforced_machine_access: Vec<String>,
}

pub(super) fn describe<P: AgentProvider + ?Sized>(
    provider: &P,
    platform: &str,
) -> NeighborConciergeCapability {
    let supported = !matches!(
        provider.system_prompt_delivery(),
        SystemPromptDelivery::External
    );
    if !supported {
        return NeighborConciergeCapability::default();
    }
    let runtime = provider.agent_runtime().is_some();
    let sandbox = provider.execution_sandbox_capability();
    let enforced_machine_access = [
        ("read_only", ExecutionIntent::ReadOnly),
        ("folder", ExecutionIntent::Mutating),
    ]
    .into_iter()
    .filter_map(|(level, intent)| {
        sandbox
            .as_ref()
            .and_then(|capability| capability.resolve(&intent, platform, runtime).ok())
            .flatten()
            .map(|_| level.to_owned())
    })
    .collect();
    NeighborConciergeCapability {
        supported,
        // The owner permits advisory confinement where the provider cannot enforce it.
        machine_access: ["read_only", "folder", "full"].map(str::to_owned).to_vec(),
        enforced_machine_access,
    }
}

#[cfg(test)]
mod tests;
