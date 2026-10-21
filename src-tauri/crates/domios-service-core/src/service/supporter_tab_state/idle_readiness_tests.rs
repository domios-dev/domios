use super::*;

#[test]
fn idle_observation_waits_until_the_authenticated_agent_ready_transition() {
    assert!(idle_observation_ready(
        true,
        false,
        Some(BootStage::AgentReady)
    ));
    for (authenticated, booting, stage) in [
        (false, false, Some(BootStage::AgentReady)),
        (true, true, Some(BootStage::AgentReady)),
        (true, false, Some(BootStage::ProviderReady)),
        (true, false, Some(BootStage::ShellReady)),
        (true, false, None),
    ] {
        assert!(!idle_observation_ready(authenticated, booting, stage));
    }
}
