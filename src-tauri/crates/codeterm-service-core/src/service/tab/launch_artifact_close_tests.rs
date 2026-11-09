use crate::agent::provider::LaunchArtifactSweepRequest;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn closing_a_codex_tab_releases_its_own_profile() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("codex".into());
    let fixture = ServiceFixture::new(mux);
    let releases = Arc::new(Mutex::new(Vec::new()));
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("codex").with_launch_artifact_logs(
            Arc::new(Mutex::new(Vec::<LaunchArtifactSweepRequest>::new())),
            releases.clone(),
        ),
    ));

    fixture.service.close_tab(&tab_id).unwrap();

    let releases = releases.lock();
    let owner = codeterm_plugin::global_writes::machine_global_write_allowed();
    assert_eq!(releases.len(), usize::from(owner));
    if owner {
        assert_eq!(releases[0].0, tab_id);
        assert!(!releases[0].1.is_empty());
    }
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_none());
}
