use super::claim_restored_pty_boot;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn fixture(owners: &[&str]) -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.protected = false;
    tab.restored_from_snapshot = true;
    tab.agent_type = None;
    tab.agent_session_id = Some("retained-session".into());
    tab.cwd = Some("D:/fixture".into());
    let fixture = ServiceFixture::new(mux);
    for owner in owners {
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(MockProvider::with_session(
                owner,
                "retained-session",
            )));
    }
    (fixture, tab_id)
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn missing_restored_provider_recovers_exact_session_before_boot_claim() {
    let (fixture, tab_id) = fixture(&["fixture-owner"]);
    fixture
        .service
        .recover_missing_restored_provider(&tab_id)
        .unwrap();
    fixture
        .service
        .recover_missing_restored_provider(&tab_id)
        .unwrap();
    let claim = claim_restored_pty_boot(&fixture.service.mux, &tab_id)
        .unwrap()
        .unwrap();
    assert_eq!(claim.provider_id, "fixture-owner");
    assert_eq!(claim.session_id.as_deref(), Some("retained-session"));
    assert!(claim_restored_pty_boot(&fixture.service.mux, &tab_id)
        .unwrap()
        .is_none());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn missing_restored_provider_refuses_ambiguous_or_absent_ownership_without_mutation() {
    for owners in [vec![], vec!["owner-one", "owner-two"]] {
        let (fixture, tab_id) = fixture(&owners);
        let before =
            serde_json::to_value(fixture.service.mux.read().to_session_snapshot()).unwrap();
        let error = fixture
            .service
            .materialize_restored_agent_for_send(&tab_id)
            .unwrap_err();
        assert!(error.starts_with("restored_provider_unresolved:"));
        let mut after =
            serde_json::to_value(fixture.service.mux.read().to_session_snapshot()).unwrap();
        after["timestamp"] = before["timestamp"].clone();
        assert_eq!(before, after);
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn missing_restored_provider_does_not_revive_a_non_restored_or_booting_tab() {
    for booting in [false, true] {
        let (fixture, tab_id) = fixture(&["fixture-owner"]);
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.restored_from_snapshot = booting;
            tab.agent_booting = booting;
        }
        fixture
            .service
            .recover_missing_restored_provider(&tab_id)
            .unwrap();
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_type
            .is_none());
    }
}
