use codeterm_types::pty::BootStage;

pub(super) fn idle_observation_ready(
    session_authenticated: bool,
    agent_booting: bool,
    boot_stage: Option<BootStage>,
) -> bool {
    session_authenticated && !agent_booting && boot_stage == Some(BootStage::AgentReady)
}

#[cfg(test)]
#[path = "idle_readiness_tests.rs"]
mod tests;
