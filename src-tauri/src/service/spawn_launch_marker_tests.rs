use super::*;
use crate::service::SpawnAgentParams;

#[test]
fn visible_agent_launch_passes_a_minted_marker_into_the_provider_launch() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "visible-marked",
    )));
    let params = SpawnAgentParams {
        agent: Some("visible-marked".into()),
        ..Default::default()
    };

    let command = visible_agent_launch_command(
        "tab-visible-marked",
        "visible-marked",
        &params,
        None,
        None,
        Some("lane-setup"),
        &registry,
    );

    let marker =
        crate::service::launch_session_detection::registered_launch_marker("tab-visible-marked")
            .expect("visible spawn registers its launch marker");
    assert!(!marker.is_empty());
    assert_eq!(
        command,
        format!("lane-setup ; visible-marked-cli --launch-marker {marker}")
    );
}
