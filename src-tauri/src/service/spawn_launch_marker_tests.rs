use super::*;
use crate::service::SpawnAgentParams;

#[test]
fn lane_provisioning_visible_launch_contains_only_agent_command() {
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
        &registry,
    );

    let marker =
        crate::service::launch_session_detection::registered_launch_marker("tab-visible-marked")
            .expect("visible spawn registers its launch marker");
    assert!(!marker.is_empty());
    assert_eq!(
        command,
        format!("visible-marked-cli --launch-marker {marker}")
    );
}
