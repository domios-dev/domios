use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn protected_general_switch_persists_defaults_and_ordinary_switch_does_not() {
    let mut mux = crate::multiplexer::state::MultiplexerState::empty();
    crate::service::general_agent::ensure_general_workspace(&mut mux, false);
    let id = mux.resolve_tab_id("general").unwrap();
    let workspace = mux.find_workspace_for_tab(&id).unwrap();
    let ordinary = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let before = fixture.service.mux.read().get_tab(&id).cloned().unwrap();
    let launch = RestorableLaunchParameters {
        model: Some("model-token".into()),
        resolved_reasoning_effort: Some("effort-token".into()),
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fixture
        .service
        .persist_general_provider_switch_with(&before, "target-token", &launch, |config| {
            std::fs::write(&path, toml::to_string_pretty(config).unwrap())
                .map_err(|error| error.to_string())
        })
        .unwrap();
    let restored: crate::config::schema::AppConfig =
        toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(restored.general_agent.agent_provider, "target-token");
    assert_eq!(restored.general_agent.model.as_deref(), Some("model-token"));
    assert_eq!(
        restored.general_agent.reasoning_effort.as_deref(),
        Some("effort-token")
    );
    let ordinary = fixture
        .service
        .mux
        .read()
        .get_tab(&ordinary)
        .cloned()
        .unwrap();
    fixture
        .service
        .persist_general_provider_switch_with(&ordinary, "other-token", &Default::default(), |_| {
            panic!("ordinary tabs cannot save General defaults")
        })
        .unwrap();
    assert_eq!(
        fixture.service.configuration().general_agent.agent_provider,
        "target-token"
    );
    let after = fixture.service.mux.read().get_tab(&id).cloned().unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.title, before.title);
    assert_eq!(after.role, before.role);
    assert_eq!(after.role_profile, before.role_profile);
    assert_eq!(
        fixture.service.mux.read().resolve_tab_id("general"),
        Some(id.clone())
    );
    assert_eq!(fixture.service.mux.read().resolve_tab_id("main"), Some(id));
}
