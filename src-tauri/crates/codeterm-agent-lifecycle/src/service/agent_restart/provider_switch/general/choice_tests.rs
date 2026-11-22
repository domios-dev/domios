//! General Agent provider-choice refusal through the in-place provider switch; moved from the
//! core provider-choice tests with the switch it exercises.

use crate::agent::test_provider::MockProvider;
use crate::prelude::*;
use crate::service::general_agent::provider_choice::test_support::profile_runtime;

#[test]
fn rejected_general_choices_leave_configuration_and_source_session_untouched() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    crate::service::general_agent::identity::apply_general_agent_kind(tab);
    tab.agent_type = Some("source".into());
    tab.agent_session_id = Some("source-session".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("opaque-adapter").with_agent_runtime(profile_runtime()),
    ));
    let config = fixture.service.ext::<crate::config::ConfigState>();
    let service = std::sync::Arc::new(fixture.service.clone());
    let before = serde_json::to_value(fixture.service.configuration()).unwrap();
    assert!(crate::service::general_agent::complete_first_run_core(
        &service,
        config,
        "opaque-adapter"
    )
    .unwrap_err()
    .starts_with("general_agent_provider_ineligible:"));
    assert!(fixture
        .service
        .switch_provider_in_place(&tab_id, "opaque-adapter", None, None, None, false)
        .err()
        .expect("switch to an ineligible provider must refuse")
        .starts_with("general_agent_provider_ineligible:"));
    assert_eq!(
        serde_json::to_value(fixture.service.configuration()).unwrap(),
        before
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("source"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("source-session"));
}
