//! Why a spawn path refuses a provider — as a value, not a sentence.
//!
//! One invariant binds every constructor here: a rejection must never present
//! the refused id as an available option. That contradiction is the bug this
//! module exists to make unrepresentable.

use std::fmt;

use super::spawn::{AgentSpawnSpec, SpawnTabSpec, TabCommand};

#[cfg(test)]
#[path = "spawn_capability/test_backend.rs"]
pub(crate) mod test_backend;

#[cfg(test)]
#[path = "spawn_capability/tests.rs"]
mod tests;

/// A launch guarantee a spawn path owes the agent it starts, which the runtime
/// the registry routed to may be unable to honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingSpawnCapability {
    /// An agent process the host launches and supervises through a PTY.
    PtyAgentRuntime,
    /// `CODETERM_WORKER_ID` in the launch env — the only handle
    /// `codeterm mem worker report` accepts.
    WorkerIdentityHandoff,
}

impl MissingSpawnCapability {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::PtyAgentRuntime => "pty_agent_runtime",
            Self::WorkerIdentityHandoff => "worker_identity_handoff",
        }
    }
}

impl fmt::Display for MissingSpawnCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PtyAgentRuntime => f.write_str(
                "this spawn launches a PTY agent runtime, which that provider does not offer",
            ),
            Self::WorkerIdentityHandoff => f.write_str(
                "this spawn issues a worker id the agent must report back with, and the routed runtime carries no worker identity",
            ),
        }
    }
}

/// A refused spawn target, split by whether listing alternatives is evidence or
/// self-contradiction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SpawnRejection {
    /// No registry knows the id. Here the available list is genuine evidence.
    UnregisteredProvider {
        requested: String,
        available: Vec<String>,
    },
    /// Registered and advertised, refused for a capability it lacks. Carries no
    /// availability list, so it can never contradict itself.
    CapabilityUnavailable {
        requested: String,
        missing: MissingSpawnCapability,
    },
}

impl SpawnRejection {
    /// The invariant, checkable on any constructed rejection.
    pub(crate) fn advertises_requested_as_available(&self) -> bool {
        match self {
            Self::UnregisteredProvider {
                requested,
                available,
            } => available.iter().any(|id| id == requested),
            Self::CapabilityUnavailable { .. } => false,
        }
    }

    pub(crate) fn missing_capability(&self) -> Option<MissingSpawnCapability> {
        match self {
            Self::UnregisteredProvider { .. } => None,
            Self::CapabilityUnavailable { missing, .. } => Some(*missing),
        }
    }

    pub(crate) fn requested(&self) -> &str {
        match self {
            Self::UnregisteredProvider { requested, .. } => requested,
            Self::CapabilityUnavailable { requested, .. } => requested,
        }
    }
}

impl fmt::Display for SpawnRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnregisteredProvider {
                requested,
                available,
            } => {
                debug_assert!(
                    !self.advertises_requested_as_available(),
                    "a rejection must never list the provider it refuses"
                );
                write!(
                    f,
                    "Unknown agent provider: '{}'. Available: [{}]",
                    requested,
                    available.join(", ")
                )
            }
            Self::CapabilityUnavailable { requested, missing } => write!(
                f,
                "Agent provider '{}' cannot serve this spawn ({}): {}",
                requested,
                missing.id(),
                missing
            ),
        }
    }
}

impl From<SpawnRejection> for String {
    fn from(rejection: SpawnRejection) -> Self {
        rejection.to_string()
    }
}

/// Classify a provider the PTY agent registry could not resolve. When the id is
/// nevertheless advertised as spawnable it is registered for a runtime this path
/// cannot drive — calling it "unknown" while listing it was the contradiction.
pub(crate) fn classify_unavailable_agent_provider(
    requested: &str,
    available: Vec<String>,
) -> SpawnRejection {
    if available.iter().any(|id| id == requested) {
        return SpawnRejection::CapabilityUnavailable {
            requested: requested.to_string(),
            missing: MissingSpawnCapability::PtyAgentRuntime,
        };
    }
    SpawnRejection::UnregisteredProvider {
        requested: requested.to_string(),
        available,
    }
}

impl SpawnTabSpec {
    /// The PTY-agent launch fields, or `None` once the registry routed this
    /// spawn to a runtime that starts no agent process to stamp them onto.
    pub(crate) fn agent_launch_mut(&mut self) -> Option<&mut AgentSpawnSpec> {
        match &mut self.command {
            TabCommand::Agent(agent) => Some(agent),
            _ => None,
        }
    }

    /// Stamp the worker identity a path is contractually required to hand out,
    /// or refuse naming the capability the routed runtime is missing.
    pub(crate) fn bind_worker_identity(
        &mut self,
        requested: &str,
        worker_id: &str,
    ) -> Result<(), SpawnRejection> {
        match self.agent_launch_mut() {
            Some(agent) => {
                agent.worker_id = Some(worker_id.to_string());
                Ok(())
            }
            None => Err(SpawnRejection::CapabilityUnavailable {
                requested: requested.to_string(),
                missing: MissingSpawnCapability::WorkerIdentityHandoff,
            }),
        }
    }
}
