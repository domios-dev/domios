use super::*;
use crate::service::SpawnAgentParams;

#[test]
fn lane_provisioning_visible_launch_is_independent_of_setup_input() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "visible-marked",
    )));
    let mut params = SpawnAgentParams {
        agent: Some("visible-marked".into()),
        ..Default::default()
    };
    params.lane_setup_command = Some("package-manager install".into());

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
    assert_eq!(command, format!("visible-marked-cli --launch-marker {marker}"));
}
